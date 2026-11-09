# MVP-0d / Lot B — overlays privés et intégration transactionnelle

> **Prototype mesuré, pas une architecture validée.** Le mode D n'a pas encore été
> comparé de bout en bout aux stratégies A/B/C sur une campagne multi-agents réelle.
> Ce document décrit ce qui est implémenté, testé, et ce qui ne l'est pas.

## 1. Rappel du problème

La campagne S8 du MVP-0c a mesuré, sur une petite fixture et trois répétitions :

| Stratégie | Médiane |
|---|---:|
| Copies privées + intégration (A) | 111 s |
| Répertoire partagé + garde de version (B) | 238 s |
| Exécution séquentielle (C) | 528 s |

Conclusion honnête : **la meilleure stratégie mesurée reste l'isolation privée**.
Aucune supériorité du répertoire partagé n'est démontrée. Le lot B construit la
troisième voie, sans prétendre qu'elle gagne déjà.

## 2. Architecture implémentée

```text
BaseSnapshot (immuable, content-adressé : SHA-256 des fichiers triés)
      |
      +-- Overlay agent A : écritures privées + suppressions
      +-- Overlay agent B
      |
      v
ChangeCoordinator.integrate(overlay)   tout ou rien
      |   - base périmée          -> RejectedStale
      |   - chemin déjà appliqué  -> ConflictPath (aucune fusion sémantique)
      |   - chemin invalide       -> RejectedInvalid
      v
snapshot_after()  ->  nouvelle base après publish()
```

Module : `crates/ravum-workspace-coordinator/src/overlay.rs`
(`BaseSnapshot`, `Overlay`, `ChangeCoordinator`, `StorageReport`).

### Règles conservatrices

1. Un agent **n'écrit jamais** la base : `Overlay::write` ne touche aucun fichier.
2. `materialize` produit une **vue de compilation** privée (base + overlay), pas un
   arbre partagé modifiable.
3. L'intégration est **tout ou rien** : si un seul chemin est en conflit, aucun
   changement de la proposition n'est appliqué.
4. Un même chemin touché par deux agents est un **conflit**, jamais une fusion
   automatique. Un CRDT convergerait peut-être ; il pourrait aussi produire un
   programme incorrect, donc il n'est pas utilisé.
5. `publish()` avance la base canonique ; toute proposition construite sur
   l'ancienne base est ensuite refusée (`RejectedStale`) et doit être rejouée.
6. Chemins validés : relatifs, séparateur `/`, aucun `..`, aucun `.`, aucun
   lecteur Windows.

## 3. Tests exécutés (9, déterministes, sans rustc ni Git)

Fichier : `crates/ravum-workspace-coordinator/tests/overlay.rs`.

| Cas | Résultat |
|---|---|
| Deux agents, fichiers indépendants | les deux acceptés, aucun fichier perdu |
| Même chemin, deux agents | `ConflictPath`, **rien** de la seconde proposition n'est appliqué |
| Base périmée après `publish()` | `RejectedStale` |
| Chemins `../`, absolus, `\`, vides | rejetés avant toute mutation |
| Agent interrompu (overlay abandonné) | base inchangée |
| 8 threads concurrents | intégration sérialisée, 8/8 acceptées, 9 fichiers finaux |
| Suppressions | appliquées, fichiers voisins conservés |
| 12 propositions indépendantes | aucune modification perdue |
| Coût de stockage | voir §4 |

## 4. Mesure de stockage (déterministe)

Base de 100 fichiers de 4 Kio (409 600 o), 8 agents écrivant chacun 2 fichiers de
16 Kio :

| Grandeur | Valeur |
|---|---:|
| Base | 409 600 o |
| Octets réellement écrits par les overlays | 262 144 o |
| Coût de 8 copies privées complètes | 3 276 800 o |
| Économie | **92,0 %** |

C'est une mesure de **duplication de stockage**, pas de temps. Elle ne dit rien de
la latence d'intégration ni du coût de vérification.

## 5. Ce qui n'est pas démontré

- **Publication Git de bout en bout** via les overlays : `GitWriter` reste
  inchangé et testé séparément (publication transactionnelle, CAS, journal). Le
  branchement direct `snapshot_after → GitWriter` n'a pas été exécuté ici.
- **Campagne A/B/C/D** sur une fixture réelle : non exécutée dans ce cycle.
- **Fusion sémantique** de deux modifications dans une même fonction : refusée par
  construction.
- **Verrouillage et reprise après crash pendant l'intégration** : le coordinateur
  est en mémoire ; la durabilité reste celle du journal de publication existant.

Ces points sont les conditions du gate « mode D supérieur » — ils restent ouverts.
