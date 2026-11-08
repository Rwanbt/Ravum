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

Sept runs de 10 à 20 itérations ont été enregistrés. Ils montrent une dispersion
importante due à la charge machine, mesurée par la calibration (`rustc -vV`).

| Run | Calibration | `total` p50 | `total` p95 |
|---|---|---|---|
| A (sans calibration) | inconnue | 380 ms | 481 ms |
| B | 83 ms | 576 ms | 778 ms |
| D | 91 ms | 716 ms | 796 ms |
| E | 99 ms | 766 ms | 908 ms |
| F | 136 ms | 902 ms | 1 218 ms |
| G (versionné) | 96 ms | 525 ms | 746 ms |

`min` observé sur le run G : **502 ms**. Détail : `evidence/m1-bench.json`,
`docs/mvp/M2-evidence.md` §6.

**Constat honnête :** ni la cible « diagnostic p95 < 50 ms » (min 132 ms) ni la cible
« petit patch p95 < 500 ms » (min 502 ms) ne sont atteintes de façon reproductible.
Le run A qui semblait passer n'est pas reproduit par les six runs instrumentés.

L'analyse invoque `rustc` depuis zéro à chaque édition : le plancher est le démarrage
du compilateur, pas l'unité analysée. C'est un argument *pour* un frontend à requêtes
persistantes (option C), mais un chiffre ne décide pas : il manque le coût de
maintenance, la précision des diagnostics et le comportement des agents sur une
syntaxe nouvelle.

**Apport de M2 à la comparaison :** le broker de preuve montre qu'un verdict borné
coûte **3 à 6 ms** (`verify_ms`, `evidence/m2-proof.json`), c'est-à-dire l'ordre de
grandeur sous lequel une analyse incrémentale native devrait se situer. Le coût de
l'outillage Rust actuel est donc intégralement dans le lancement du compilateur.

**Apport de M3 :** le harnais DSP mesure 1–2 µs (p95) pour traiter 256 frames, soit
environ **0,04 %** d'une période de 5 333 µs, avec **0 allocation** sur le thread
audio (`docs/mvp/M3-evidence.md`). Ce n'est pas une mesure de frontend, mais cela
donne l'ordre de grandeur du budget restant si une analyse incrémentale native devait
tourner à côté du thread audio.

**Apport de M4 :** la verticale full-stack n'apporte **aucun chiffre de compilation** :
l'UI est statique et le serveur est un binaire Rust ordinaire. Elle montre en revanche
ce que l'option C devrait rendre incrémental — contrat de types partagé, migration de
schéma, validation — et **aucune de ces trois choses n'est mesurée en temps de
compilation**. C'est une lacune de mesure, pas un résultat.

## 6. Ce qui manque pour ouvrir le gate

| Élément | État |
|---|---|
| S1-R (baseline Rust complète jusqu'au linking) | **partiel** : M1 mesure ce chemin |
| S1-T (mini frontend à requêtes) | **non commencé** (#65) |
| S2..S9 | **non exécutés** |
| S8-A / S8-B (productivité agent IA) | **non exécutés** (#33, #68) |
| Oracle sémantique indépendant de rustc | **non défini** (#66) |
| Décision humaine | **en attente** (#69) |
