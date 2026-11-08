# Commencer ici — première itération Ravum

> **Aucune implémentation commencée.** Ce document est l'ordre suggéré d'ouverture des premières PR ; pas un compte-rendu d'exécution.

## Première vague (M0)

1. [#24 RAV-001](https://github.com/Rwanbt/Ravum/issues/24) — traçabilité P0/ADR/tests et revue de couverture.
2. [#26 RAV-003](https://github.com/Rwanbt/Ravum/issues/26) — référence matérielle 16 Go et protocole de benchmark.
3. [#27 RAV-004](https://github.com/Rwanbt/Ravum/issues/27) — corpus de référence (calculator, DSP, full-stack, négatifs), avant tout benchmark.
4. [#28 RAV-005](https://github.com/Rwanbt/Ravum/issues/28) et [#29 RAV-006](https://github.com/Rwanbt/Ravum/issues/29) — identités, protocole et états structurés (proposés).
5. [#33 RAV-010](https://github.com/Rwanbt/Ravum/issues/33) — pilote de productivité IA et test hors corpus.
6. [#32 RAV-009](https://github.com/Rwanbt/Ravum/issues/32) — baseline Rust S1-R jusqu'au linking.
7. [#36 RAV-013](https://github.com/Rwanbt/Ravum/issues/36) — critères de décision M5 pré-enregistrés.

## Organisation des travaux

Créer des branches de tâche **depuis `dev`** et des PR **vers `dev`**, en indiquant le numéro GitHub, les résultats observés et les tests manquants. Limiter les conflits : une même frontière de module a un propriétaire de travail pour une PR donnée.

La première PR de documentation peut définir les schémas et le corpus sans introduire de runtime. La première PR de code du MVP ne doit créer que des composants avec consommateurs réels. Pas de tests E2E ignorés pour paraître plus rapide.

## Gates

- **M0** : corpus et mesures reproductibles.
- **M1** : boucle source→diagnostic→patch sûr ; refus démontré, pas seulement succès.
- **M2** : preuve/caches invalidés correctement, sans `PROVED` synthétique.
- **M3** : tests RT réels + limitations matérielles déclarées.
- **M4** : E2E UI/API/SQLite.
- **M5** : comparaison A/B/C et décision humaine. Aucun choix de nouveau langage forcé avant les résultats.

Lire [MVP](MVP.md), [backlog](BACKLOG.md) et [AGENTS](../../AGENTS.md).
