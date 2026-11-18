# Décisions du responsable — 2026-11-18

> Décisions prises par le responsable du dépôt en réponse aux questions posées
> après la fusion de `dev` dans `main` (PR #151). Elles s'appliquent dès leur
> enregistrement. Les règles non négociables d'E6 restent en vigueur, sauf la
> règle 8 levée ci-dessous.

## Objectif

**Toutes les gates T0–T12 doivent être `PASS`.** Les étapes E7 à E10 du plan de
production doivent être terminées. Les gates qui exigent une action humaine
restent ce qu'elles sont : un agent prépare leur dossier et ne les promeut pas.

| Gate | Action humaine requise |
|---|---|
| T1 | Ratification de l'ADR de backend |
| T4, T10 | Audit externe indépendant |
| T11 | Custody de la clé de signature, ou confirmation de P-10 |
| T12 | Go/No-Go |

## D-18-1 — Règle 8 levée

**Avant :** aucun cas de conformance n'était ajouté depuis Linux.

**Maintenant :** des cas peuvent être écrits depuis Linux. Un cas ne compte
dans une gate que s'il passe en CI sur Linux, macOS et Windows.

**Motif :** T2 exige un FeatureId et un cas pour chacun des 344 énoncés MUST
sans test.

## D-18-2 — Périmètre de la 1.0 élargi

Les décisions **A2** (bloc DSP reporté à la 1.1) et **P-13** (périmètre 1.0
gelé) sont **annulées**. La 1.0 inclut les fonctionnalités qui manquent à T6,
T8 et T9 :

- concurrence (threads, async) ;
- réseau et HTTP ;
- SQLite ;
- hot reload avec préservation d'état ;
- bloc DSP.

Chaque fonctionnalité commence par un RFC `PROPOSED`. Aucune n'est implémentée
avant que son RFC soit écrit. Aucune release n'a lieu avant que T6, T8 et T9
soient `PASS`.

## D-18-3 — Canal de dépréciation

DIAG-1.6 est étendu à un troisième avertissement : **`W-DEPRECATED`**. Il est
réservé à la dépréciation et encadré par
[`DEPRECATION-POLICY.md`](../release/DEPRECATION-POLICY.md). Aucune construction
n'est dépréciée en 1.0, mais le canal existe et il est testé.

## D-18-4 — Alarme de temps : 20 échantillons en CI

Le job `performance.yml` mesure 20 exécutions par exemple et par mode au lieu
de 10. Avec 10 échantillons, le p95 au rang le plus proche est le maximum, et un
seul pic du runner partagé faisait échouer le job (PR #169). Les plafonds et la
référence ne changent pas. La comparaison avec la référence, mesurée avec 10
échantillons, devient un peu plus indulgente sur le p95 ; le responsable l'a
accepté le 2026-11-18.

## D-18-5 — `run --json`

DIAG-8.1 renvoie à `run --json` (`status`, `exit_code`). La commande est
implémentée, au lieu de retirer la phrase de la spécification.

## Ce qui ne change pas

- Aucun tag, aucune release, aucune signature, aucune fusion vers `main` sans
  l'accord explicite du responsable.
- Une PR par étape, vers `dev`, CI verte avant fusion.
- Aucun test, seuil ni contrat n'est affaibli. Une SKIP ou un NOT_RUN n'est
  jamais un PASS.
