# Contribuer à Ravum

> **Licence du projet : `MIT OR Apache-2.0`**, au choix du réutilisateur. Toute contribution destinée à être intégrée doit pouvoir être publiée sous les deux licences (voir [LICENSE-MIT](LICENSE-MIT) et [LICENSE-APACHE](LICENSE-APACHE)). Les contributeurs conservent leurs droits sur leurs apports ; ils doivent disposer des autorisations nécessaires et signaler tout composant ou contenu tiers avec sa licence. Toute attribution ou mention de droits antérieure reste applicable.

## Branches et flux

- `main` est la branche par défaut et la référence documentaire revue.
- `dev` est la branche d'intégration des expérimentations et des fonctionnalités.
- Créer `feat/<issue>-slug`, `fix/<issue>-slug`, `research/<issue>-slug` **depuis `dev`**, puis ouvrir une PR **vers `dev`**. La promotion vers `main` suit une PR de release documentaire ou qualifiée distincte, jamais un push aveugle.
- Lier l'issue et expliciter les ADR concernés. Petites PR, testables et isolées.

## Définition de terminé (DoD)

Chaque modification doit fournir le scénario testé, les tests négatifs, la configuration/outils, les limites, les résultats mesurés (si revendiqués) et un rapport sur la traçabilité P0→ADR→test. Les garanties inconnues ne sont pas un succès.

Ne pas désactiver un test E2E défaillant pour contourner un problème. Les tests par impact doivent être conservateurs ; une suite intégrale périodique vérifie les oublis d'impact.

## ADR et agents IA

- Toutes les décisions ADR sont `PROPOSED` jusqu'à approbation explicite.
- Un agent peut **proposer** un amendement, jamais l'accepter, supprimer un test, affaiblir un contrat ou prétendre avoir exécuté un benchmark.
- Les revues d'ADR doivent citer le texte de décision réel (pas seulement le titre).
- Une nouvelle idée passe par une RFC pour éviter les collisions d'identifiants.

## Sécurité

Aucun secret, identifiant privé ni donnée de production dans une issue, un patch ou une trace. Consultez [SECURITY.md](SECURITY.md). Limiter les dépendances et scripts de build ; ne pas lancer de logiciel non fiable hors sandbox.

## Avant d'ouvrir une PR

Vérifier le code adapté au périmètre, la documentation, les tests négatifs, les changements de schéma et les hypothèses. Si seule la documentation existe, la CI ne prouve **aucune** qualité d'exécutable.
