# M5 — Critères de décision pré-enregistrés (langage autonome ?)

> **Ce document ne décide rien.** Il fige les critères *avant* que les expériences
> comparatives soient exécutées, pour éviter qu'un résultat soit interprété après
> coup. La décision appartient au responsable du projet ; aucun agent ne peut la
> prendre. Aucun ADR n'est accepté ici.

## 1. Les trois options comparées

| Option | Description | Ce que ce serait |
|---|---|---|
| **A** | Rust existant + outillage Ravum | l'état actuel du MVP-0a |
| **B** | mini-frontend Ravum → Rust | un sucre syntaxique compilé vers Rust |
| **C** | mini-frontend Ravum → IR propre → Cranelift | le début d'un langage autonome |

## 2. Protocole

- corpus **identique** pour A, B, C (mêmes fonctionnalités, mêmes tests) ;
- même machine, même toolchain de référence, mêmes modèles IA ;
- chaque mesure accompagnée du SHA, de la version d'outil, de l'OS et de la config ;
- les expériences S1-R (Rust baseline) et S1-T (frontend jouet) sont **distinctes**
  et ne se valident pas l'une l'autre.

## 3. Métriques obligatoires

| Métrique | Définition | Pourquoi |
|---|---|---|
| `T_analyze` | édition → diagnostic affichable | P0-A |
| `T_build` | édition → artefact chargeable | P0-A |
| `T_activate` | artefact → comportement visible | P0-A |
| `T_total` | demande → livrable vérifié | P0-I (critère dominant) |
| `RSS_daemon` | mémoire du processus permanent | P0-H |
| `CPU_idle` | CPU au repos du daemon | P0-H |
| `L_agent` | nombre d'itérations de correction par un agent IA | P0-E |
| `T_agent` | temps total agent jusqu'à correctif vérifié | P0-I |
| `G_gap` | garanties réellement imposées (et non revendiquées) | P0-D |
| `M_maint` | coût de maintenance du frontend (taille, complexité, tests) | durabilité |

## 4. Règles de décision (pré-enregistrées)

1. **A reste la référence** tant que B ou C ne démontre pas un gain mesuré sur
   `T_total` **ou** `T_agent`, sans dégradation de `G_gap`.
2. Un gain inférieur à **10 %** sur `T_total` ne justifie pas le coût de `M_maint`
   d'un frontend autonome.
3. Une amélioration de `L_agent` due à une syntaxe nouvelle doit être reproduite
   sur du code **hors corpus** (S8-B), sinon elle est traitée comme un artefact
   d'entraînement.
4. Aucun choix de langage n'est accepté sur la base d'un score théorique ou d'une
   préférence esthétique.
5. Si B et C échouent, l'outillage Rust (A) reste le produit ; ce n'est pas un échec
   du projet, c'est un résultat.

## 5. Données déjà disponibles (hors comparaison M5)

Le MVP-0a fournit des mesures réelles, mais **elles ne comparent pas A/B/C** : elles
caractérisent le chemin A sur une seule fonction `f64 -> f64`.

| Mesure (20 itérations) | p50 | p95 | p99 |
|---|---|---|---|
| analyse (`check`) | 103 ms | 136 ms | 171 ms |
| build + link | 269 ms | 356 ms | 366 ms |
| vérification smoke | 3 ms | 4 ms | 4 ms |
| chargement natif | 2 ms | 4 ms | 4 ms |
| activation | 0 ms | 0 ms | 0 ms |
| **total** | **380 ms** | **481 ms** | **544 ms** |

Détail et limites : `evidence/m1-bench.json`, `docs/mvp/M1-evidence.md`.

**Constat honnête :** la cible exploratoire « diagnostic local p95 < 50 ms » n'est
**pas** atteinte (139 ms). L'analyse invoque `rustc` depuis zéro à chaque édition,
sans daemon ni cache de requêtes. C'est un argument *pour* un frontend à requêtes
persistantes (option C), mais un seul chiffre ne décide pas : il manque le coût de
maintenance, la précision des diagnostics et le comportement des agents sur une
syntaxe nouvelle.

## 6. Ce qui manque pour ouvrir le gate

| Élément | État |
|---|---|
| S1-R (baseline Rust complète jusqu'au linking) | **partiel** : M1 mesure ce chemin |
| S1-T (mini frontend à requêtes) | **non commencé** (#65) |
| S2..S9 | **non exécutés** |
| S8-A / S8-B (productivité agent IA) | **non exécutés** (#33, #68) |
| Oracle sémantique indépendant de rustc | **non défini** (#66) |
| Décision humaine | **en attente** (#69) |
