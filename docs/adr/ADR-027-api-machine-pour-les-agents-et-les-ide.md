# ADR-027 — API machine pour les agents et les IDE

- **Statut** : `PROPOSED` — validation humaine et preuves en attente.
- **Date de proposition** : 2026-10-23.
- **Décideur** : à désigner.
- **Dépendances** : ADR-014, ADR-019.
- **Impact** : architecture du compilateur, profil cible, sécurité et/ou expérience développeur selon le domaine.

## Contexte et problème

Les diagnostics texte libres font gaspiller des tokens et compliquent les corrections.

## Décision proposée

API JSON/SARIF versionnée, IDs de diagnostics/obligations, liens code→contrat→preuve→test→binaire, diff et contre-exemple minimal, CLI/LSP indépendants IDE.

## Alternatives à comparer / écarter

Sortie CLI humaine seulement ; RPC propriétaire non versionné ; agent directement lié au compilateur.

## Conséquences et compromis

+ Agent automation reproductible. - Versionner les schémas et compatibilité.

## Preuves à produire avant acceptation

Tests de contrat API, mutation des champs, compression de feedback mesurée et compatibilité OpenCode/Unifia/VS Code via clients.

**Critère d'acceptation** : expérience reproductible, SHA + plateforme + toolchain + résultats + limites documentées, sans fail critique inexpliquée. Mettre à jour l'ADR avec chiffres mesurés avant le passage à `ACCEPTED`.

## Échec, repli et conditions de réouverture

La taille et complexité des événements dépassent leur intérêt.

Si la preuve échoue : statut maintenu `PROPOSED` ou `REJECTED`, preuve jointe, alternative expérimentée ; **ne pas dégrader les contrats exigés** pour faire passer l'essai.

## Questions ouvertes

- Quelles hypothèses sont confiées à des outils ou runtimes tiers ?
- Quels cas limites doivent produire `UNSUPPORTED`/`UNKNOWN` plutôt qu'un succès ?
- Quelles mesures sur plateforme de référence pourraient réfuter ce choix ?
- Quels compromis de maintenance et d'ergonomie devront être revus à T1/T2 ?

## Historique

- 2026-10-23 — Proposition initiale à discuter.
