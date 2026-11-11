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
5. `publish()` avance la base canonique. Il **clôt le cycle d'intégration** :
   `applied` est vidé, et la comptabilité `total_overlay_bytes`/`agents` repart à
   zéro ; `history` est conservé comme journal d'audit. Sans cette remise à zéro,
   une proposition construite sur la **nouvelle** base et touchant un chemin
   modifié au cycle précédent était refusée à tort en `ConflictPath` (défaut P0
   corrigé, couvert par un test de régression à deux cycles).
6. Chemins validés strictement : relatifs, séparateur `/`, aucun `.`/`..`, aucun
   antislash, aucun lecteur Windows/UNC/verbatim, aucun `:` (flux alternatif NTFS),
   aucun caractère de contrôle, aucun nom réservé Windows (même avec extension),
   aucun composant vide ni point/espace final, longueurs bornées (1024 octets par
   chemin, 255 par composant).
7. `materialize` revalide chaque chemin, canonicalise la racine, refuse tout
   composant symlink existant, écrit atomiquement (temporaire dans le répertoire
   cible + `rename`) et applique une police de tailles (100 000 fichiers, 1 Gio par
   défaut). Les limites TOCTOU résiduelles sont documentées dans le module : le
   module empêche un snapshot/chemin hostile de sortir de la racine, il ne prétend
   pas résister à un processus local qui remplace un répertoire par un lien entre
   deux appels système.

## 3. Tests exécutés

### 3.1 Overlays (17, déterministes, sans rustc ni Git)

Fichier : `crates/ravum-workspace-coordinator/tests/overlay.rs`.

| Cas | Résultat |
|---|---|
| Deux agents, fichiers indépendants | les deux acceptés, aucun fichier perdu |
| Même chemin, deux agents | `ConflictPath`, **rien** de la seconde proposition n'est appliqué |
| Base périmée après `publish()` | `RejectedStale` |
| **Deux cycles : A publié, B sur la nouvelle base touche le même chemin** | `Applied` (régression P0 : plus de fausse conflit) |
| **CAS conservée après publication** | ancienne base → `RejectedStale`, nouvelle base → `Applied` |
| Chemins `../`, absolus, `\`, vides | rejetés avant toute mutation |
| **Corpus hostile élargi** (`C:\`, `\\?\`, `CON.txt`, `a:b`, `a.`, `a `, contrôle, trop long) | tous refusés par `validate_relative_path`, `Overlay::write`/`delete` et `BaseSnapshot::from_files` |
| **Symlink de répertoire/fichier existant** | `materialize` refuse, aucun octet hors racine (SKIP explicite si Windows refuse la création du lien) |
| **Limites de matérialisation** | dépassement fichiers/octets → `InvalidInput` |
| Agent interrompu (overlay abandonné) | base inchangée |
| 8 threads concurrents | intégration sérialisée, 8/8 acceptées, 9 fichiers finaux |
| Suppressions | appliquées, fichiers voisins conservés |
| 12 propositions indépendantes | aucune modification perdue |
| Coût de stockage | voir §4 |

### 3.2 Cycle transactionnel (14, dépôts Git jetables)

Fichier : `crates/ravum-workspace-coordinator/tests/cycle_integration.rs`,
API `commit_cycle` (overlay → oracle → `GitWriter` → `publish`).

| Cas | Résultat |
|---|---|
| Cycle nominal | un commit, base avancée, arbre/index réconciliés |
| Rejeu du même `publication_id` | `AlreadyPublished`, aucun second commit |
| Crash avant le swap (INTENT seul) | `recover()` → reprise, puis **une** publication |
| Crash après le swap (sans OUTCOME) | `RECOVERED_PUBLISHED`, rejeu refusé |
| INTENT dans un état que la reprise ne peut pas expliquer | refus (`Journal`), jamais rejoué en aveugle |
| Base ≠ `HEAD` sur un chemin modifié | `StaleBase`, rien de publié |
| `expected_head` déplacé | `ConcurrentWriter`, rien de publié |
| Deux cycles concurrents | exactement un publié, l'autre refusé, dépôt `fsck` sain |
| Refus sémantique de l'oracle | aucun état fantôme, aucune publication |
| Crochet `pre-commit` qui refuse | aucun état adopté, rejeu unique après retrait |
| Contrat impacté | `HumanRequired` (INV-02) |
| Demande incohérente / sans journal | `InvalidRequest` |
| Suppression publiée | fichier absent de l'arbre et du disque |


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

- **Campagne A/B/C/D** sur une fixture réelle : non exécutée dans ce cycle. Le
  branchement `overlay → snapshot_after → oracle → GitWriter → publish` est
  désormais implémenté et testé (`commit_cycle`) sur des dépôts jetables, mais
  aucune mesure de latence ou de débit de bout en bout n'est établie.
- **Fusion sémantique** de deux modifications dans une même fonction : refusée par
  construction.
- **Verrou inter-processus** : le coordinateur d'overlays reste en mémoire. Deux
  cycles concurrents sont départagés par le CAS de `GitWriter` et le journal
  durable (un seul publie), mais il n'existe pas de verrou couvrant toute la durée
  du cycle ; le CAS base ↔ `HEAD` et le CAS de référence sont les points d'échec
  fail-closed.
- **TOCTOU** : `materialize` refuse les liens existants et canonicalise la racine,
  mais ne peut pas empêcher un processus local hostile de remplacer un répertoire
  par un lien entre deux appels système (`std` stable n'expose pas `openat`/
  `O_NOFOLLOW`). Cette limite est explicite dans le module.
- **Contenu non UTF-8** : le modèle `SourceSnapshot` est textuel ; `commit_cycle`
  échoue fermé (`VerificationUnavailable`) plutôt que de publier sans vérification.

