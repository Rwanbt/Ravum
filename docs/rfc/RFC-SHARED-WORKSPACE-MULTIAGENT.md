# RFC-SHARED-WORKSPACE-MULTIAGENT (proposition, non ratifiée)

> **Encadré d'en-tête**
>
> | Champ | Valeur |
> |---|---|
> | Statut | `PROPOSED` — aucune ratification, aucune acceptation implicite |
> | Date | 2026-11-08 |
> | Numéro ADR | **aucun attribué**. Ne pas écrire « ADR-042 » ni aucun identifiant dérivé |
> | Prototype | **COLLAB-1 partiel** : `crates/ravum-workspace-coordinator` — propositions, conflits, journal, écrivain Git unique. Ni rebase automatique, ni AST, ni CRDT, ni mesure de coût |
> | Mesures | **aucune mesure CPU/RAM/latence**. Tous les chiffres sont des cibles de conception à réfuter |
> | Autorité | Le mainteneur humain. Un agent ne peut ni ratifier ni marquer cette RFC comme décision |
> | Objet | Coordination d'agents IA et d'humains écrivant du code dans un espace partagé, sans conflit silencieux ni perte d'intention |
> | Documents reliés | `PROJECT_INVARIANTS.md`, `specs/03-compilation-continue-hot-swap.md`, ADR-008, ADR-012, ADR-021, ADR-022, ADR-027, ADR-041, `plan/00-plan-directeur.md` |
> | Gouvernance | La consolidation du 2026-11-08 §8.1 impose « pas de multi-agent simultané sur les mêmes fichiers ». **Cette RFC ne l'abroge pas** ; elle décrit ce qu'il faudrait prouver avant d'envisager un assouplissement |
>
> **Ce que ce document n'est pas** : une preuve, un benchmark, une décision d'architecture, ni une autorisation d'implémenter. C'est un dossier de conception falsifiable destiné à être attaqué.

---

## 1. Problème et besoins utilisateurs

### 1.1 Situation observée

Plusieurs agents IA et un ou deux humains modifient le même projet en parallèle. Les stratégies disponibles sont toutes insatisfaisantes :

| Stratégie | Coût réel | Défaillance caractéristique |
|---|---|---|
| Un worktree par agent | Checkout complet, cache de compilation et dépendances dupliqués | Caches redondants ; l'agent travaille sur un état invisible des autres ; l'intégration est un saut dans l'inconnu |
| Une branche par agent, un checkout | Changements de branche destructifs, conflits d'index | État de travail écrasé ; `checkout` concurrent corrompt l'index |
| Édition directe du même répertoire | Rapide à démarrer | Dernier écrivain gagne silencieusement ; aucune détection de perte |
| Sérialisation humaine | Sûr | Ne passe pas à l'échelle ; le temps total jusqu'au livrable vérifié (P0-I) reste dominé par l'attente |

Sur une machine de référence à 16 Go, cinq worktrees Rust avec chacun leur `target/` consomment plusieurs gigaoctets et saturent la RAM pendant les builds concurrents. La duplication dégrade directement P0-H (sobriété) et P0-I (temps total).

### 1.2 Le problème réel n'est pas le merge

Le merge textuel est le symptôme. Le problème de fond est triple :

1. **Incertitude sur l'état partagé.** Un agent ignore quel est l'état canonique et si son observation est encore valide.
2. **Perte silencieuse d'intention.** Deux modifications peuvent être « fusionnées » en un fichier syntaxiquement valide, compilable et sémantiquement faux, sans aucune erreur.
3. **Invalidation non fiable.** Un résultat produit sur un état ancien peut être réutilisé comme s'il portait sur l'état courant — exactement ce qu'interdisent INV-04 et INV-07.

Un quatrième problème est souvent négligé : **l'attribution**. Sans réponse à « quelle proposition, de quel agent, sur quelle tâche, depuis quel snapshot a produit ces octets ? », ni l'audit (P0-D), ni le rollback, ni la mesure du temps agent (P0-I) ne sont possibles.

### 1.3 Besoins, formulés comme des tests

| ID | Besoin | Formulation falsifiable |
|---|---|---|
| B-01 | Ne perdre aucune modification acceptée | Aucune séquence concurrente ne produit un état publié où une proposition `VALIDATED` a disparu sans état terminal |
| B-02 | Savoir sur quel état on travaille | Toute proposition nomme le snapshot observé ; tout résultat nomme le snapshot concerné |
| B-03 | Détecter le conflit sémantique | Un merge textuellement propre qui change une signature utilisée ailleurs est au minimum `REBASE_REVALIDATE` |
| B-04 | Ne pas dupliquer les caches | Les agents partagent le stockage de blobs et les artefacts adressés par contenu |
| B-05 | Rester réactif | La coordination n'introduit pas de latence perceptible dans frappe→diagnostic |
| B-06 | Pouvoir refuser | En cas de doute, refus et conservation du dernier état valide ; jamais de « réparation » aveugle |
| B-07 | Auditer | Chaque octet publié est rattachable à `proposal_id`, `agent_id`, `task_id`, `base_snapshot_id` |
| B-08 | Ne dépendre d'aucun service | Fonctionnement local, hors ligne, sans compte cloud |

### 1.4 Contrainte de gouvernance

La consolidation multi-IA du 2026-11-08 (§8.1) énonce : « Pas de multi-agent simultané sur les mêmes fichiers ; travail par branches/worktrees et propriétaires des sous-systèmes ; intégration testée au SHA. » Cette règle est **conservatrice et correcte par défaut**. Elle reste l'état applicable tant que les conditions de §14 ne sont pas remplies.

---

## 2. Invariants de sécurité

Numérotés localement (`SW-*`) pour ne pas collisionner avec `INV-01…INV-13`.

### SW-01 — Aucune écriture concurrente non coordonnée des mêmes octets

Obtenu par construction, pas par verrou : les agents n'écrivent jamais dans l'arbre canonique. Ils écrivent des blobs immuables dans un store adressé par contenu et soumettent des propositions. Un seul composant matérialise le monde canonique.
**Test de réfutation** : instrumenter tous les `open(O_WRONLY)` sur l'arbre canonique ; un seul processus doit y écrire, quel que soit le nombre d'agents.

### SW-02 — Aucune perte silencieuse d'intention

Toute proposition atteint un état terminal observable : `PUBLISHED`, `REJECTED`, `SUPERSEDED`, `ABANDONED`. Une proposition `REJECTED` conserve son corps, son motif et sa base dans un journal append-only. Un état intermédiaire orphelin est un défaut bloquant.
**Test de réfutation** : tuer un agent à un instant arbitraire ; sa proposition est absente (jamais reçue) ou terminale, jamais orpheline.

### SW-03 — Un seul écrivain Git

Un seul composant exécute `add`, `commit`, `checkout`, `switch`, `reset`, `rebase`, `merge`, `update-ref`, `gc`, `prune`. Les agents n'ont aucun accès en écriture à `.git` ni à l'index.
**Test de réfutation** : permissions FS + audit des processus ; toute tentative d'écriture `.git` par un agent échoue et est journalisée.

### SW-04 — Rejet des propositions périmées

Une proposition dont `base_snapshot_id` n'est pas l'état courant n'est jamais appliquée telle quelle : elle est classée puis, si rebasée, revalidée. Aucun chemin de code n'applique un patch à un blob dont le hash diffère de la préimage déclarée.
**Test de réfutation** : modifier la préimage d'un seul octet ; l'application échoue avant toute écriture.

### SW-05 — Refus fail-closed

Toute incertitude produit un refus : symbole introuvable, hash absent, dépendance non déclarée détectée, fichier généré modifié, contrat impacté non déclaré, politique de chemin indisponible, store indisponible, horloge incohérente. Le dernier état valide est conservé.
**Test de réfutation** : injecter une faute dans chaque sous-système ; aucun chemin ne produit d'état publié partiel.

### SW-06 — Traçabilité par proposition

L'unité de traçabilité est la **ChangeProposal**, pas le fichier ni le commit. Chaque publication porte l'identité de sa proposition ; chaque commit porte cette identité en trailer. La chaîne `proposition → snapshot validé → commit → artefact → activation` est reconstructible hors ligne.
**Test de réfutation** : depuis un commit arbitraire, retrouver proposition, agent, tâche, base et classe de fusion sans base volatile.

### SW-07 — Aucune activation d'un snapshot non validé

Un snapshot publié n'est pas activable. L'activation reste soumise à ADR-021 et à la spec 03 : patch préparé, ABI compatible, vérifié pour son profil, bascule atomique à un safepoint, ancienne version conservée jusqu'à quiescence. Cette RFC ne crée **aucun** chemin d'activation parallèle.
**Test de réfutation** : activer un snapshot dont le check a échoué ou dont une preuve requise est `UNKNOWN` en mode strict → refus, ancienne version active.

### SW-08 — Monotonie de l'état canonique

L'état canonique ne recule jamais. Une publication produit un `SnapshotId` dont la base est l'ancien. Un retour arrière est une nouvelle proposition (revert), jamais une réécriture.
**Test de réfutation** : la suite des snapshots publiés forme une chaîne sans bifurcation ni cycle.

### SW-09 — Déterminisme de la classification

Pour un même état canonique, une même base et un même corps de proposition, la classe et la décision sont identiques. Toute dépendance à l'horloge murale, à l'ordre d'arrivée ou à l'identité de l'agent dans la décision est un défaut.
**Test de réfutation** : rejouer le journal dans un ordre d'arrivée différent ; décisions identiques par proposition.

### SW-10 — Bornes de ressources et absence de famine

Aucune proposition ne peut être affamée indéfiniment. Files bornées, admission équitable, et au-delà d'un seuil d'attente une escalade observable (`MANUAL_REQUIRED` ou notification), jamais un silence.
**Test de réfutation** : soumettre des `AUTO_SAFE` prioritaires en continu pendant que des `CONFLICT` attendent ; mesurer l'attente maximale et vérifier l'escalade.

---

## 3. Taxonomie de la concurrence

La confusion la plus coûteuse est de traiter « même branche », « même copie physique » et « mêmes octets » comme synonymes.

### 3.1 Les sept situations

| # | Situation | Même octet physique ? | Risque principal | Mécanisme requis |
|---|---|---|---|---|
| C1 | Même branche, worktrees distincts | Non | Course sur la référence ; un commit en écrase un autre | CAS sur la ref + écrivain unique |
| C2 | Même branche, même copie physique | Oui | Écrasement de l'état de travail, corruption d'index | Interdiction d'écriture directe ; overlay |
| C3 | Fichiers distincts | Non | Quasi nul ; risque résiduel sur générés, manifests, lockfiles | Fusion par chemin, `AUTO_SAFE` après check |
| C4 | Fonctions distinctes d'un même fichier | Oui | Hunks disjoints mais contexte trompeur ; imports partagés | Merge 3 voies + analyse de région ; `REBASE_REVALIDATE` |
| C5 | Même fonction | Oui | Un des deux corps disparaît et le résultat compile | `CONFLICT` systématique |
| C6 | Modules dépendants (A change une API, B l'utilise) | Non | Merge propre, compilation cassée ou sémantique changée sans erreur | Graphe de dépendances + contrats ; `REBASE_REVALIDATE` ou `MANUAL_REQUIRED` |
| C7 | Écritures Git concurrentes | — | Index corrompu, ref perdue, historique réécrit | Écrivain unique + `commit-tree` + `update-ref` CAS |

### 3.2 Conséquences

**C1 n'implique pas C3.** Deux agents sur la même branche dans deux worktrees écrivent des octets différents mais se disputent la même référence. Le worktree ne résout donc pas la concurrence : il déplace le conflit du fichier vers la ref. C'est le point que les stratégies « un worktree par agent » passent sous silence.

**C2 est le cas réellement dangereux** — plusieurs workers dans un checkout partagé. La seule défense robuste est structurelle : les agents n'écrivent pas dans l'arbre canonique. Une discipline « je ne touche qu'à mes fichiers » est une convention sociale, pas un invariant ; elle ne résiste ni à une erreur d'agent, ni à un outil qui écrit un cache au mauvais endroit (`.rustc_info.json`, lockfile, artefact).

**C4 est le piège du merge textuel.** Deux hunks disjoints peuvent produire un fichier valide et faux : deux agents ajoutent chacun un `use` et modifient des appels différents, mais l'un change la visibilité d'un item que l'autre utilise. Git ne voit rien.

**C6 est le piège du compilateur.** Un changement d'API dans A et un appel dans B fusionnent sans conflit. Si l'invalidation est correcte, on obtient une erreur de compilation — acceptable. Si elle est trop fine, on obtient un binaire qui compile mais dont le comportement a changé : c'est la régression la plus coûteuse, et elle justifie que `impacted_contracts` soit obligatoire.

### 3.3 Niveau minimal imposé

| Situation | Niveau minimal |
|---|---|
| C3 | `AUTO_SAFE` (avec check) |
| C4 | `REBASE_REVALIDATE` |
| C5 | `CONFLICT` |
| C6 | `REBASE_REVALIDATE` ; `MANUAL_REQUIRED` si signature publique ou contrat |
| C1 | Sérialisation par l'écrivain unique ; aucun merge Git implicite |
| C2 | Impossible par construction (overlay) |
| C7 | Impossible par construction (écrivain unique) |

---

## 4. Modèle de snapshots, overlays et propositions

### 4.1 Principe

L'état canonique est un **arbre de Merkle** : chaque fichier est un blob identifié par le hash de son contenu ; la racine est un manifeste trié associant chemin normalisé, mode, type et hash. Un snapshot est immuable : le modifier signifie en créer un nouveau. Un agent ne modifie jamais un snapshot ; il produit un **overlay** de blobs nouveaux et un **ChangeSet** (une ou plusieurs **ChangeProposals** décrivant la transformation relativement à un snapshot de base).

### 4.2 Architecture candidate

```text
                 ┌──────────────────────────────────────────────┐
                 │  CANONICAL SOURCE STATE                      │
                 │  arbre de Merkle : path -> blob_hash         │
                 │  + manifestes + fichiers générés déclarés    │
                 └───────────────────┬──────────────────────────┘
                                     │ hash(manifeste canonique)
                                     ▼
                 ┌──────────────────────────────────────────────┐
                 │  IMMUTABLE SNAPSHOT  Sn = sha256:...         │
                 │  SourceSnapshotId (indépendant de rustc)     │
                 └───────┬──────────────────────────┬───────────┘
                         │ lecture seule            │ lecture seule
                         ▼                          ▼
       ┌───────────────────────────┐   ┌───────────────────────────┐
       │ OVERLAY agent A           │   │ OVERLAY agent B           │
       │ blobs nouveaux            │   │ blobs nouveaux            │
       │ base_snapshot_id = Sn     │   │ base_snapshot_id = Sn     │
       └─────────────┬─────────────┘   └─────────────┬─────────────┘
                     │ ChangeProposal Pa              │ Pb
                     └───────────────┬────────────────┘
                                     ▼
                 ┌──────────────────────────────────────────────┐
                 │  WORKSPACE COORDINATOR                       │
                 │  seul propriétaire de l'état canonique       │
                 │  journal append-only des propositions        │
                 │  ordonnanceur équitable, files bornées       │
                 │  AUCUNE écriture Git ici                     │
                 └───────────────────┬──────────────────────────┘
                                     ▼
                 ┌──────────────────────────────────────────────┐
                 │  DEPENDENCY / CONFLICT GRAPH                 │
                 │  nœuds : fichier, symbole, module, contrat   │
                 │  arêtes : importe, appelle, implémente,      │
                 │           satisfait, génère                  │
                 │  parcourt UNIQUEMENT le voisinage des edits  │
                 └───────────────────┬──────────────────────────┘
                                     ▼
             ┌─────────────┬─────────────┬──────────────┬─────────────┐
             ▼             ▼             ▼              ▼
       ┌──────────┐  ┌───────────┐ ┌────────────┐ ┌──────────────┐
       │ REBASE   │  │ MERGE     │ │ REJECT     │ │ MANUAL       │
       │ (3-way)  │  │ (chemins  │ │ (fail-     │ │ REQUIRED     │
       │          │  │ disjoints)│ │  closed)   │ │ (humain)     │
       └────┬─────┘  └─────┬─────┘ └─────┬──────┘ └──────┬───────┘
            └───────┬──────┘             │               │
                    │  fusion / retry    ▼               ▼
                    ▼             [ REJECTED ]   [ décision humaine ]
       ┌───────────────────────────────────────────────────────────┐
       │  VALIDATED SNAPSHOT  Sv                                   │
       │  merge appliqué + préconditions vérifiées                 │
       └───────────────────────────┬───────────────────────────────┘
                                   ▼
       ┌───────────────────────────────────────────────────────────┐
       │  INCREMENTAL CHECK + TESTS IMPACTÉS                       │
       │  requêtes invalidées ciblées (ADR-012)                    │
       │  preuves requises selon profil                            │
       │  échec => pas de publication, dernier bon état conservé   │
       └───────────────────────────┬───────────────────────────────┘
                                   ▼
       ┌───────────────────────────────────────────────────────────┐
       │  ATOMIC PUBLISH                                           │
       │  nouveau Sn+1, événement SnapshotPublished                │
       │  visible de tous ; activation NON automatique             │
       └───────────────────────────┬───────────────────────────────┘
                                   ▼
       ┌───────────────────────────────────────────────────────────┐
       │  SINGLE GIT WRITER                                        │
       │  commit-tree + update-ref (CAS sur la ref)                │
       │  trailers Ravum-Proposal / Ravum-Agent / Ravum-Task       │
       │  jamais de reset --hard, jamais de rebase d'historique    │
       └───────────────────────────┬───────────────────────────────┘
                                   ▼
                             [ commit Git ]
```

### 4.3 Pourquoi cette forme

**Snapshot immuable en amont** : le coordinateur n'a pas à protéger un état mutable contre les lectures ; tout le monde lit le même objet figé, et SW-01 devient structurel plutôt que défensif. **Overlay côté agent** : travail sur un état incomplet sans polluer le canonique, conformément à M1 du `MVP.md`. **Graphe de conflits borné** : on ne compare que le voisinage des chemins et symboles touchés, sinon le coût devient quadratique et P0-H tombe. **Validation avant publication, publication avant Git** : Git est un journal de sortie, pas une primitive de concurrence ; l'utiliser comme coordination revient à confier la correction sémantique à un outil de diff textuel.

### 4.4 Identités

| Identité | Calcul | Propriété |
|---|---|---|
| `BlobId` | `sha256(bytes)` | Adressage par contenu, déduplication naturelle |
| `SourceSnapshotId` | `sha256(manifeste canonique trié)` | Indépendant de la toolchain, de l'OS, du profil |
| `ProposalId` | `sha256(corps canonique)` | Identité de l'intention, pas de l'auteur |
| `ValidatedSnapshotId` | `sha256(manifeste après merge validé)` | Distinct de `SourceSnapshotId` : porte le résultat |
| `BuildContextId` | `SourceSnapshotId` + toolchain + flags + cible | Conforme à la séparation des cinq identités |
| `CommitSha` | Git | Sortie, jamais entrée de décision |

La séparation `SourceSnapshotId` / `BuildContextId` / `VerificationKey` / `ArtifactId` / `ActiveVersionId` est reprise de la consolidation (§4) et de `MVP.md` M0. Cette RFC ne la redéfinit pas.

---

## 5. ChangeProposal

### 5.1 Champs normatifs

| Champ | Type | Sémantique | Interdit |
|---|---|---|---|
| `agent_id` | string + `instance_id` | Identité de l'auteur. Ne sert **jamais** à la décision de fusion, seulement à l'attribution et à l'anti-auto-approbation | Anonyme |
| `task_id` | string | Corrélation avec l'issue et le registre de preuves ; permet d'agréger le temps total | Absent |
| `base_snapshot_id` | `sha256:` | Snapshot exact observé. Condition nécessaire de toute application | « dernier » ou vide |
| `target_paths` | liste triée de chemins normalisés | Fichiers que la proposition prétend modifier. Toute écriture hors liste est un refus | Vide ou glob non résolu |
| `target_symbols` | liste d'identifiants symboliques | Symboles dont la sémantique change. Sert au graphe de conflits et au périmètre d'invalidation | Absent si `target_paths` non vide |
| `expected_hashes` | map `path -> blob_hash` | Préconditions CAS : hash observé de chaque cible à la base | Absent |
| `patch` | diff unifié avec préimage par hunk | Transformation ; application exacte ou échec | Patch sans préimage |
| `declared_dependencies` | liste de symboles/modules/contrats | Ce que la proposition suppose stable. Rend la dépendance vérifiable | Vide (la vérification s'élargit, jamais ne se rétrécit) |
| `impacted_contracts` | liste d'identifiants de contrats | Contrats dont obligations, pré/postconditions, invariants ou effets peuvent changer. Liste vide = assertion « aucun », vérifiable | Absent |

### 5.2 Sémantique des champs critiques

**`base_snapshot_id`** — pivot du modèle. Il n'identifie pas « à peu près cet état » mais un manifeste exact. Une base périmée ne rend pas la proposition invalide, mais elle n'est plus applicable directement : rebase ou rejet, jamais application telle quelle.

**`expected_hashes`** — primitive de concurrence optimiste. Rend la proposition falsifiable : si le hash observé ne correspond pas au blob présent, l'agent a travaillé sur une observation périmée. Redondant avec `base_snapshot_id` par construction, mais conservé pour (a) vérifier sans charger le snapshot complet, (b) diagnostiquer précisément quel fichier a changé, (c) détecter une incohérence de store.

**`target_symbols`** — identité stable et indépendante de la position : `module_path::item_path#kind` avec désambiguïsateur, plus une ancre de secours `(base_snapshot_id, path, span)`. Si le symbole ne se résout plus (renommage par un autre agent), la proposition passe en `CONFLICT` ou `MANUAL_REQUIRED`, jamais en application silencieuse par position.

**`declared_dependencies`** — deux rôles. Positif : restreindre le périmètre de revalidation, donc le coût. Négatif : si le vérificateur découvre une dépendance non déclarée effectivement impactée, c'est le signal que l'agent n'a pas compris son changement. Politique recommandée : `REBASE_REVALIDATE` avec périmètre élargi et avertissement, pas rejet immédiat — un rejet pousserait les agents à déclarer « tout », ce qui détruit le bénéfice.

**`impacted_contracts`** — champ le plus sensible. Il ne donne pas le droit de modifier un contrat ; il déclare qu'un contrat est touché, ce qui déclenche `MANUAL_REQUIRED` si une obligation est retirée, affaiblie ou modifiée. Un agent qui déclare `[]` alors qu'un contrat est affecté commet une fausse déclaration : c'est un défaut à détecter par test, pas une opinion.

### 5.3 Enveloppe opérationnelle

| Champ | Rôle |
|---|---|
| `intent` | Description courte pour l'humain et le board d'intentions |
| `created_at` | Horodatage déclaratif ; jamais utilisé dans la décision |
| `policy_profile` | Profil d'obligations (`default_safe`, `verified`, `rt_safe`…) |
| `budget` | Plafonds CPU/RAM/temps acceptés pour la validation |
| `supersedes` | `proposal_id` antérieur remplacé explicitement |
| `evidence_refs` | Diagnostics, tests ou preuves déjà produits |

### 5.4 Cycle de vie

```text
DRAFT ─submit─> RECEIVED ─schema─> CLASSIFIED
                        │              ├─ AUTO_SAFE ────────────┐
                        │              ├─ REBASE_REVALIDATE ───┤
                        │              ├─ CONFLICT ──> (attente)│
                        │              ├─ MANUAL_REQUIRED ──┐   │
                        │              └─ REJECT ──┐        │   │
                        ▼                          ▼        ▼   ▼
                   INVALID_SCHEMA              REJECTED  (humain)
                                                    ▲
              merge + check + tests ◄───────────────┘
                        │
             ┌──────────┴──────────┐
             ▼                     ▼
        VALIDATED             FAILED_VALIDATION
             │                     │
             ▼                     ▼
        PUBLISHED             REJECTED (motif)
```

Chaque transition écrit une entrée dans le journal append-only. Le journal est la vérité ; l'état courant est une projection.

---

## 6. Niveaux de fusion

Les cinq niveaux ne sont pas des degrés de « confiance » mais des **régimes de traitement** avec des obligations distinctes.

### 6.1 Définitions

| Niveau | Définition | Obligations avant publication |
|---|---|---|
| `AUTO_SAFE` | Aucune interaction possible : chemins disjoints, aucun symbole partagé, aucun contrat impacté, aucun fichier généré | Application exacte + check ciblé sur les fichiers touchés + tests des modules touchés |
| `REBASE_REVALIDATE` | Interaction possible ou avérée, résolution mécanique possible, sémantique à re-prouver | Rebase 3 voies + revalidation élargie aux dépendances déclarées et découvertes + tests impactés |
| `CONFLICT` | Composition mécanique impossible : hunks chevauchants, même fonction modifiée, symbole cible supprimé ou renommé | Aucune publication automatique ; reformulation sur la nouvelle base |
| `REJECT` | Violation dure : schéma invalide, chemin protégé, généré non déclaré, préimage absente ou fausse, écriture hors `target_paths`, cycle introduit, budget dépassé | Aucune écriture ; motif structuré obligatoire |
| `MANUAL_REQUIRED` | Résoluble mécaniquement, mais l'autorité est humaine : retrait/affaiblissement de contrat, changement d'ABI ou de layout, migration d'état, module sensible, intentions incompatibles sur un symbole | Décision humaine explicite, tracée, justifiée |

**Point essentiel** : `AUTO_SAFE` ne signifie pas « non vérifié » mais « périmètre de vérification étroit et sûr ». Aucun niveau ne publie sans check.

### 6.2 Procédure de classement

L'ordre des prédicats est normatif : il garantit le déterminisme (SW-09) et empêche qu'un cas grave soit masqué par un cas bénin.

```text
ENTRÉE : proposition P, état canonique HEAD (snapshot Sn_head)

1. Schéma et politique
   schéma invalide                              -> REJECT(SCHEMA)
   chemin protégé                               -> REJECT(PROTECTED_PATH)
   écriture hors target_paths                   -> REJECT(OUT_OF_SCOPE)
   généré/lockfile non déclaré                  -> REJECT(GENERATED_FILE)
   bornes de taille/durée dépassées             -> REJECT(BUDGET)

2. Préconditions
   base absente du store                        -> REJECT(UNKNOWN_BASE)
   un hash de expected_hashes ne correspond pas -> REJECT(CAS_MISMATCH)

3. Contrats
   impacted_contracts non vide ET retrait /
   affaiblissement / changement d'obligation    -> MANUAL_REQUIRED(CONTRACT)
   impacted_contracts vide MAIS contrat affecté -> MANUAL_REQUIRED(UNDECLARED_CONTRACT)

4. Base courante
   si P.base_snapshot_id == Sn_head :
      application exacte (préimage par hunk)
        échec                                    -> REJECT(PATCH_APPLY)
      signature publique / ABI / layout change   -> MANUAL_REQUIRED(ABI)
      sinon                                      -> AUTO_SAFE

   sinon (base périmée) :
      aucun target_path modifié depuis la base   -> REBASE_REVALIDATE(TRIVIAL)
      merge 3 voies (base, head, P) :
        hunks chevauchants                       -> CONFLICT
        même fonction touchée des deux côtés     -> CONFLICT
        symbole cible supprimé/renommé           -> CONFLICT
      merge propre :
        changement de (base..head] touchant un
        target_symbol ou une declared_dependency -> REBASE_REVALIDATE(SYMBOL)
        chemins disjoints et aucun symbole
        partagé                                  -> AUTO_SAFE
        sinon                                    -> REBASE_REVALIDATE(OVERLAP)

5. Validation
   check incrémental + tests impactés + preuves du profil
     échec                                       -> FAILED_VALIDATION -> REJECTED
     succès                                      -> VALIDATED -> PUBLISHED
```

### 6.3 Cas limites explicites

| Cas | Classement | Justification |
|---|---|---|
| Deux agents ajoutent un `use` différent en tête du même fichier | `REBASE_REVALIDATE` | Le merge réussit mais l'ordre et les imports globaux affectent la résolution |
| Deux fonctions distinctes, l'un change la visibilité de l'autre | `REBASE_REVALIDATE` | La visibilité est une interface ; le merge ne la voit pas |
| Renommage d'une fonction d'un côté, appel de l'autre | `CONFLICT` | Le symbole cible n'existe plus sous son identité déclarée |
| `Cargo.lock` modifié pendant qu'un autre ajoute une dépendance | `REJECT` puis `MANUAL_REQUIRED` | Fichier généré ; la résolution est une décision, pas un merge |
| Suppression d'un `pub fn` inutilisé localement | `MANUAL_REQUIRED` | Suppression d'interface publique, même si le check local passe |
| Faute de frappe dans un commentaire pendant une réécriture de la fonction | `CONFLICT` | Chevauchement de contexte ; la résolution automatique n'est pas justifiable |

---

## 7. Alternatives techniques étudiées

Chaque alternative est évaluée sur ce qu'elle **ne** résout pas.

### 7.1 CRDT (Automerge, Yjs)

**Résout** : convergence garantie et déterministe de structures répliquées, sans coordination centrale, avec fusion commutative.
**Ne résout pas** : la correction sémantique. Un CRDT garantit que tous les réplicas convergent vers **le même** document — pas que ce document est du code valide, qu'il compile, ni que les contrats sont préservés. Deux agents peuvent converger parfaitement vers une fonction qui ne fait plus ce que l'un voulait. Un CRDT sur un AST ne change rien : la convergence est une propriété de la structure, pas du sens.
**Coût** : tombstones et horloges vectorielles croissantes, compaction, intégration Git non triviale (le CRDT devrait devenir source de vérité, contre SW-03), débogage difficile.
**Verdict** : utile pour un éditeur collaboratif, **inadapté** comme mécanisme de publication de code.

### 7.2 OT (Operational Transformation)

**Résout** : transformation d'opérations concurrentes pour préserver l'intention locale sur du texte, avec serveur autoritaire.
**Ne résout pas** : OT est conçu pour un document plat. Aucune notion de symbole, de type, d'interface ni de contrat. Les propriétés de préservation d'intention supposent un modèle que le code ne respecte pas.
**Coût** : algorithmes de transformation délicats, sensibles à l'ordre, historiquement sujets à des bugs subtils ; serveur autoritaire obligatoire, donc un coordinateur réintroduit — sans les garanties sémantiques.
**Verdict** : complexité élevée pour un bénéfice limité au cas « plusieurs curseurs dans le même fichier ». Ne pas retenir.

### 7.3 MVCC

**Résout** : lectures non bloquantes sur versions cohérentes, écritures concurrentes avec détection de conflit à la validation, isolation par transaction. Conceptuellement très proche du modèle proposé.
**Ne résout pas** : MVCC détecte les conflits d'**écriture** sur les mêmes objets, pas les conflits d'**intention**. Deux transactions sur des objets différents dont les effets se contredisent passent. Il suppose un gestionnaire qui connaît les objets ; ici les frontières fichier/symbole sont poreuses.
**Coût** : rétention des versions, purge, transactions longues (un agent travaille des minutes), risque de blocage de la purge et de croissance mémoire.
**Verdict** : **le modèle le plus proche de la cible**. Cette RFC en est une spécialisation : snapshots immuables + validation optimiste + committer unique. On garde l'idée, on rejette la généralité.

### 7.4 Concurrence optimiste / CAS

**Résout** : exactement le cœur du lost update. `compare-and-swap` sur un hash est simple, déterministe, sans verrou, testable. C'est la primitive retenue par `expected_hashes`.
**Ne résout pas** : le CAS dit « les octets n'ont pas changé », pas « mon changement reste correct ». Il ne détecte pas les conflits entre fichiers distincts.
**Coût** : quasi nul par fichier. Le coût apparaît pour un CAS multi-fichiers cohérent : il faut une racine, donc un snapshot.
**Verdict** : **retenu** comme primitive de base, combiné au snapshot pour l'atomicité multi-fichiers.

### 7.5 Event sourcing

**Résout** : journal append-only, reconstruction d'état, audit complet, replay — conforme à SW-02 et SW-06.
**Ne résout pas** : le replay n'est pas déterministe si les événements dépendent de l'extérieur (toolchain, horloge, réseau). Ne résout pas la validation sémantique. Un journal sans compaction devient un coût.
**Coût** : infrastructure de journal, schémas versionnés, snapshots pour éviter le replay intégral, migration de schéma.
**Verdict** : **retenu partiellement** — journal des propositions et publications, compaction par snapshot, pas d'event sourcing intégral.

### 7.6 Content-addressable storage

**Résout** : déduplication, immuabilité, intégrité vérifiable, partage de caches. Répond à B-04 et SW-08.
**Ne résout pas** : ni la concurrence d'écriture ni la sémantique. Ne protège pas d'un blob référencé mais corrompu sur disque sans vérification à la lecture.
**Coût** : GC des blobs non référencés, gestion documentée des collisions, coût de hachage à chaque écriture.
**Verdict** : **retenu** comme substrat. C'est le disque du modèle, pas son intelligence.

### 7.7 Merge sémantique / AST-aware

**Résout** : détecter des conflits que le texte masque (renommage, déplacement, visibilité) et proposer des résolutions structurellement valides.
**Ne résout pas** : l'équivalence sémantique. Deux AST distincts peuvent être équivalents ; deux AST identiques peuvent différer par le contexte (résolution de noms, traits, features). Un AST valide ne prouve pas un programme correct.
**Coût** : parseur tolérant, représentation stable, merge 3 voies sur arbre. Développement élevé, exécution modérée. Dépend de la qualité du parseur : s'il échoue sur du code incomplet, le merge est inutilisable.
**Verdict** : **différé** à COLLAB-3, quand un parseur incrémental stable existe. Pas un prérequis de COLLAB-1.

### 7.8 Arbres syntaxiques persistants

**Résout** : partage structurel de mémoire, comparaison en O(diff), pas de reparse complet. **Ne résout pas** : la concurrence — un arbre persistant facilite le calcul de différences mais ne décide pas si deux différences sont compatibles. **Coût** : structures sophistiquées, sérialisation, compatibilité de format ; bénéfice surtout mémoire, pas correction. **Verdict** : **pertinent pour le compilateur incrémental**, orthogonal à cette RFC, à ne pas confondre avec un mécanisme de fusion.

### 7.9 Typed patching

**Résout** : un patch typé échoue si les types ne correspondent pas, au lieu de produire un fichier douteux. **Ne résout pas** : un patch bien typé peut violer une postcondition, et il exige un langage et un typeur stables — inexistants pour Ravum aujourd'hui. **Coût** : très élevé tant que la sémantique n'est pas figée ; pour Rust en MVP-0a, il faudrait passer par rust-analyzer et ses limites d'API. **Verdict** : **cible à long terme** ; le patch textuel avec préimage et vérification post-merge est le compromis honnête.

### 7.10 Index Git et transactions de références

**Résout** : `git update-ref` avec ancienne valeur est un CAS natif ; `commit-tree` crée un commit sans toucher l'index ; `GIT_INDEX_FILE` isole une transaction.
**Ne résout pas** : pas de transaction atomique universelle multi-refs (les mécanismes existants ne couvrent pas index et worktree ensemble). Git ne sait rien de la sémantique.
**Coût** : faible, outillage standard. Risque : les commandes de haut niveau manipulent index et worktree de façon non atomique — il faut descendre au plumbing.
**Verdict** : **retenu pour l'écrivain unique uniquement**, en plumbing strict.

### 7.11 Jujutsu (jj)

**Résout** : commits immuables et anonymes, working copy comme commit, conflits de première classe, pas d'index — conceptuellement très proche de « snapshot immuable + overlay ».
**Ne résout pas** : jj manipule des versions et des conflits **textuels** ; il ne connaît ni contrats, ni types, ni symboles, ne fournit pas de validation sémantique ni de coordination multi-agents (plusieurs `jj` sur le même repo retombent sur des problèmes de working copy).
**Coût** : adopter jj changerait les habitudes Git, la CI et les protections de branche ; les gates d'ADR-031 seraient à réécrire.
**Verdict** : **référence de conception, pas dépendance**. On emprunte le commit immuable et la working copy dérivée.

### 7.12 Synthèse

| Alternative | Convergence | Correction sémantique | Git | Coût | Retenu |
|---|---|---|---|---|---|
| CRDT | Oui | **Non** | Non | Élevé | Non |
| OT | Oui (texte) | **Non** | Non | Élevé | Non |
| MVCC | Oui | Non | Non | Moyen | Principe |
| CAS optimiste | Partiel | Non | Non | Faible | **Oui** |
| Event sourcing | Non | Non | Non | Moyen | Journal |
| CAS storage | Non | Non | Non | Faible | **Oui** |
| Merge AST | Non | Partiel | Non | Élevé | Différé |
| Arbres persistants | Non | Non | Non | Moyen | Orthogonal |
| Typed patching | Non | Partiel | Non | Très élevé | Différé |
| Git plumbing | Non | Non | **Oui** | Faible | **Oui** |
| Jujutsu | Oui | Non | Autre modèle | Moyen | Référence |

**Conclusion** : aucune alternative ne résout le problème complet. La combinaison retenue est CAS de stockage + snapshots immuables + propositions optimistes + graphe de dépendances + classification + écrivain Git unique. Le reste est raffinement futur (merge AST) ou piège (CRDT comme source de vérité).

---

## 8. Réponses aux douze questions

### Q1 — Fusionner au niveau texte, AST, symboles ou IR ?

Les quatre, à des moments différents. **Symboles** : première barrière, seul niveau qui détecte « tu as changé la signature que j'utilise » sans parseur parfait, et qui fonctionne sur du code incomplet si l'index est tolérant. **Texte** : mécanisme d'application et de diff ; nécessaire parce que c'est ce qui finit dans le fichier et que c'est robuste au code invalide. **AST** : amélioration de précision, pas fondation ; réduit les faux conflits et les faux propres. **IR** : inadaptée à la fusion — l'IR est une projection après analyse, et la remontée vers la source n'est pas bijective (optimisations, désucrage, macros). L'IR sert à **vérifier**, pas à **fusionner**. Un merge qui ne consulte que l'IR ne peut pas produire de source ; un merge qui ne consulte que le texte perd l'intention.

### Q2 — Quelle granularité, à quel coût ?

Granularité **symbole** pour la décision, **fichier** pour l'application, **hunk** pour le diagnostic. Décider au symbole évite de déclarer en conflit deux fonctions distinctes d'un même fichier, tout en captant deux modifications du même symbole. Appliquer au fichier est nécessaire : un patch partiel n'a pas de sens si les hunks sont interdépendants (imports, ordre de déclaration). Diagnostiquer au hunk donne un retour actionnable. Le coût est dominé par la résolution symbolique, pas par le diff. Si l'index est déjà maintenu par le compilateur incrémental, le coût marginal est faible ; s'il faut reparser, il devient prohibitif et la granularité doit retomber au fichier. **Règle** : ne jamais promettre une granularité que l'index ne fournit pas fiablement — une granularité surestimée produit des `AUTO_SAFE` faux, pire qu'un `CONFLICT` inutile.

### Q3 — Comment un agent voit-il les changements des autres ?

Par événements de publication et relecture du snapshot courant, jamais par surveillance de fichiers. Le coordinateur émet `SnapshotPublished { snapshot_id, previous, changed_paths, changed_symbols, proposal_id }`. Un agent peut s'abonner et rebaser, ou ignorer et voir sa proposition passer en `REBASE_REVALIDATE`. Un **board d'intentions** consultatif (« B travaille sur `parser::expr` ») évite le travail redondant ; il n'est **jamais** un mécanisme de correction : il peut être périmé, menteur ou absent. La visibilité est *pull* avec notification *push* optionnelle : un agent non abonné reste correct, simplement moins efficace. Si la correction dépendait d'un événement, une perte de message produirait une perte de modification.

### Q4 — Comment empêcher les lost updates ?

Trois barrières indépendantes, redondantes volontairement : (1) précondition CAS sur `expected_hashes` ; (2) base explicite `base_snapshot_id` égale à l'état courant, sinon rebase ou rejet ; (3) application exacte, chaque hunk portant le hash de sa préimage. La troisième est la plus importante : elle rend impossible l'application « approximative » sur un contenu différent. Un `git apply --3way` peut réussir sur un contenu modifié ; ici, non. **Cas résiduel** : deux modifications successives du même fichier, chacune avec une base valide. La seconde est rebasée ; si le merge est propre et le check passe, elle est publiée. Il n'y a pas de lost update, mais il peut y avoir une **régression sémantique** que seul le test détecte (scénario T11) — risque assumé, pas problème résolu.

### Q5 — Comment préserver les contrats ?

En traitant `impacted_contracts` comme une déclaration falsifiable et en refusant toute modification automatique d'obligation. Un contrat est identifié, versionné, avec une empreinte de ses obligations. Déclaration non vide + changement de précondition, postcondition, invariant ou effet ⇒ `MANUAL_REQUIRED`, sans exception. Déclaration vide + impact détecté ⇒ `MANUAL_REQUIRED` + avertissement, pas rejet silencieux. Un contrat **renforcé** n'est pas automatiquement acceptable : il peut invalider des appelants. La règle est donc « tout changement d'obligation est manuel », et non « affaiblissement = refus, renforcement = OK ». Cela découle de INV-02 : l'IA propose un changement de spécification, elle ne l'accepte pas.

### Q6 — Comment rester efficace en compilation incrémentale ?

En faisant du snapshot l'unité d'identité des requêtes. Chaque clé inclut le hash du fichier et les interfaces dont la requête dépend. Un merge produit un diff de manifeste ; seuls les fichiers dont le hash change sont modifiés. `target_symbols` et `declared_dependencies` restreignent le périmètre initial ; le vérificateur peut l'élargir. Un résultat produit sur un snapshot non ancêtre de l'état courant est `STALE` et ne peut pas être publié (INV-07). Le point délicat est l'interface publique : un changement de signature invalide tous les dépendants, ce qui est coûteux mais correct. La mitigation est de rendre ces changements rares et explicites (`MANUAL_REQUIRED`), pas de les rendre indétectables.

### Q7 — Que faire des signatures publiques ?

Les traiter comme des contrats de première classe. Une modification est détectée en comparant les `target_symbols` à l'index d'interface du snapshot de base. Elle force au minimum `REBASE_REVALIDATE` avec périmètre élargi à tous les dépendants connus ; un retrait ou un changement incompatible est `MANUAL_REQUIRED`. Un ajout de paramètre avec défaut, ou une nouvelle fonction, n'est pas un retrait : `REBASE_REVALIDATE` si des dépendants existent, `AUTO_SAFE` sinon. **Limite honnête** : la détection dépend de la qualité de l'index. Pour Rust en MVP-0a, rust-analyzer couvre une partie avec des trous connus (macros, génériques complexes, traits). Ces trous doivent produire `UNKNOWN` et un classement conservateur, jamais un `AUTO_SAFE` optimiste.

### Q8 — Fichiers générés et lockfiles ?

Jamais de fusion automatique, jamais de merge textuel. Un fichier généré est **déclaré** dans le manifeste ; non déclaré, il est traité comme source ordinaire — ce qui est un piège. Politique : modification d'un généré ⇒ `REJECT(GENERATED_FILE)` avec instruction de régénérer ; l'agent modifie la source du générateur ou le manifeste, pas la sortie. Les lockfiles sont un cas particulier : la résolution est une **décision** (choix de version) ; deux résolutions concurrentes ne se fusionnent pas, elles se remplacent après décision. Seule voie acceptable : un agent propose un changement de manifeste, un job dédié régénère le lockfile de façon déterministe, et le résultat est vérifié comme sortie de générateur.

### Q9 — Deadlocks et famine ?

Par construction, **aucun verrou de fichier détenu par un agent** : les agents soumettent et repartent ; l'attente est asynchrone. Les seuls verrous sont internes au coordinateur, courts et non imbriqués. Les validations longues (tests, preuves) se font hors section critique, sur un snapshot figé : le coordinateur ne détient rien pendant ce temps. L'équité est assurée par une file FIFO par classe avec quantum, et un seuil d'attente déclenche une escalade, pas un abandon silencieux. **Risque résiduel** : famine par rebase infini. Mitigation : après N rebases (par exemple 3), la proposition est **gelée** sur un snapshot de référence, validée contre lui, puis publiée avec un merge explicite si nécessaire. Le gel est observable et tracé.

### Q10 — Historique et rollback ?

L'historique est append-only ; le rollback est une nouvelle entrée. Un snapshot publié n'est jamais supprimé ni modifié ; un retour arrière produit un nouveau snapshot au contenu antérieur. Git suit la même règle : pas de `reset --hard` sur une branche partagée, pas de `rebase` d'historique publié, pas de `push --force` ; un rollback est un commit de revert. Chaque commit porte les trailers permettant de retrouver la proposition annulée. L'activation (ADR-021) est distincte de la publication : revenir à un état est une publication, l'activer est une décision séparée soumise aux mêmes vérifications. Les effets externes irréversibles ne sont pas annulés par un rollback (ADR-022), et l'interface doit le dire.

### Q11 — Écrire dans Git sans conflit d'index ?

En n'utilisant jamais l'index partagé et uniquement le plumbing :

```text
1. arbre    : git mktree / git update-index --index-info, GIT_INDEX_FILE dédié
2. commit   : git commit-tree <tree> -p <parent> -m <message> (trailers Ravum-*)
3. ref      : git update-ref refs/heads/<branch> <new> <old>   (CAS)
4. échec CAS: relire la ref, re-classifier, retenter ou abandonner
```

Aucun `add`, `commit`, `checkout`, `switch`, `reset`, `rebase`, `merge` de haut niveau. `GIT_INDEX_FILE` dédié par transaction empêche deux écritures de se corrompre mutuellement. Le CAS sur la ref est la dernière barrière : même avec deux coordinateurs par erreur, un seul gagnerait. L'arbre est construit depuis les blobs du store, pas depuis le worktree : le worktree n'est jamais source de vérité pour un commit, et peut rester sale sans affecter l'intégrité. Sa resynchronisation est une opération séparée, non critique, hors transaction de publication.

### Q12 — Réutilisable pour Rust et TypeScript avant le langage Ravum ?

Oui, si le noyau est séparé de l'adaptateur de langage.

| Couche | Rust | TypeScript | Spécifique Ravum |
|---|---|---|---|
| Store de blobs, snapshots, manifestes | Oui | Oui | Non |
| ChangeProposal, CAS, journal | Oui | Oui | Non |
| Coordinateur, ordonnanceur | Oui | Oui | Non |
| Écrivain Git | Oui | Oui | Non |
| Graphe de dépendances | Interface | Interface | Implémentation |
| Résolution de symboles | rust-analyzer/LSP | tsserver/LSP | Native |
| Classification de conflit | Squelette | Squelette | Règles |
| Contrats | Non | Non | Oui |

Le noyau (quatre premières lignes) est agnostique. C'est ce qui permet de le valider **avant** Ravum : sur un dépôt Rust réel avec rust-analyzer, et un dépôt TypeScript avec tsserver. Les règles de conflit et l'analyse de contrats restent derrière une interface. **Limite honnête** : sans contrats natifs, la préservation des obligations ne peut pas être démontrée sur Rust ou TypeScript. On peut démontrer coordination, détection textuelle et symbolique, invalidation, atomicité et traçabilité — pas la préservation sémantique. Cette partie reste `UNKNOWN` jusqu'à l'existence d'un langage avec contrats, ou l'acceptation de contrats externes (attributs, décorateurs) comme approximation.

---

## 9. Relation avec Git

### 9.1 Répartition des rôles

| Composant | Rôle Git | Interdits |
|---|---|---|
| Agent | Lecture (`show`, `cat-file`) pour comprendre l'historique | Toute écriture `.git` ; `add/commit/checkout/switch/reset/rebase/merge/stash` |
| Coordinateur | Propriétaire unique des mutations | Réécriture d'historique, force-push, contournement des protections |
| CI | Lecture au SHA exact | Toute écriture |

### 9.2 Modèle de commit

Un commit contient un changement publié, ou un ensemble atomique de propositions validées **comme un tout**.

```text
<type>(<scope>): <résumé>

<body — justification, référence à l'issue>

Ravum-Proposal: sha256:9f2c...
Ravum-Agent: worker-f/instance-3
Ravum-Task: #3
Ravum-Base-Snapshot: sha256:41ab...
Ravum-Merge-Class: REBASE_REVALIDATE
Ravum-Contracts: none | [api::payments::charge]
```

Les trailers sont la source de l'attribution. Un commit sans `Ravum-Proposal` est humain ou hors protocole : valide, mais non attribuable au coordinateur. Le message ne contient jamais de données sensibles (chemins absolus, secrets, contenu de fichiers). `Ravum-Merge-Class` documente la décision et permet d'auditer les `AUTO_SAFE` a posteriori pour détecter une dérive sur des modules sensibles.

### 9.3 CAS sur la référence

`git update-ref` avec `<old>` est la primitive : si la ref a bougé, la commande échoue. Le coordinateur relit la ref, vérifie que le nouveau head descend de la base utilisée, re-classifie les propositions en attente, retente avec un nombre borné de tentatives, puis escalade (`MANUAL_REQUIRED`) en conservant le dernier état. Aucune boucle infinie : l'échec est borné et observable.

### 9.4 Ce que Git ne fait pas

Git ne détecte pas les conflits sémantiques, ne connaît pas les contrats, ne garantit pas qu'un merge compile, ne garantit pas l'atomicité index+worktree+refs, et ne fournit pas d'identité de snapshot indépendante de la toolchain. C'est un **journal d'octets** avec un bon outillage de diff, utilisé comme tel.

### 9.5 Branches

| Usage | Politique |
|---|---|
| `main` | Protégée, référence documentaire, pas une preuve de maturité |
| `dev` | Intégration ; les publications y atterrissent |
| Branche de travail | Optionnelle ; le coordinateur en est le seul écrivain |
| Branche par agent | **Déconseillée** : réintroduit la duplication et le coût d'intégration que cette RFC cherche à éviter |

---

## 10. Compatibilité avec le compilateur incrémental

### 10.1 Principe

Le snapshot est l'entrée du compilateur — pas le worktree, pas l'horloge, pas un chemin. Cela aligne le modèle de concurrence sur le modèle d'identité du compilateur (ADR-012, ADR-013) et évite une classe entière de bogues d'invalidation.

### 10.2 Clés de requête

```text
QueryKey =
  ( item_id
  , kind                      # parse | type | ownership | effects | contract | codegen
  , file_blob_hash            # contenu exact
  , interface_hashes[]        # interfaces des dépendances directes
  , contract_hashes[]         # obligations dont dépend la requête
  , policy_profile
  , toolchain_id
  , flags_hash
  )
```

L'inclusion de `file_blob_hash` rend la clé insensible au chemin et au timestamp. Deux snapshots qui partagent un blob partagent les résultats ; un merge qui ne change pas un blob ne l'invalide pas.

### 10.3 Invalidation ciblée après merge

```text
diff = { (path, old_hash, new_hash) }
changed_symbols = resolve(diff)                    # via l'index symbolique
impacted = changed_symbols
         ∪ closure(changed_symbols, declared_dependencies)
         ∪ closure(changed_symbols, interface_dependents)   # si signature publique
invalidate(impacted); recompute(impacted)
```

Le `closure` sur les dépendants d'interface est **conservateur** : en cas de doute il élargit, jamais ne restreint. Si la résolution symbolique échoue, l'invalidation porte sur le fichier entier, voire le module — plus cher et correct. Les résultats `STALE` ne sont pas supprimés : marqués, conservés pour le diagnostic, jamais publiés comme courants.

### 10.4 Rejet des résultats périmés

Un résultat est acceptable si et seulement si (1) son `SourceSnapshotId` est un ancêtre de l'état courant **et** (2) sa `QueryKey` est identique dans l'état courant. La condition 1 seule ne suffit pas (un fichier peut changer puis rechanger) ; la 2 seule non plus (un résultat peut être correct pour une clé mais produit sous une politique différente). Un job dont le snapshot n'est plus un ancêtre est annulé si possible, ignoré sinon, marqué `STALE` avec motif, et ne peut jamais écraser un résultat plus récent (INV-07).

### 10.5 Coût de l'intégration

Le coût principal est la résolution symbolique après merge, qui exige un index à jour. Si l'index est maintenu par le compilateur incrémental, le coût marginal est proportionnel au changement, pas à la taille du dépôt ; s'il doit être reconstruit, le coût devient linéaire et la promesse de réactivité tombe. **Conséquence** : l'index symbolique doit être un composant persistant et partagé, pas un sous-produit jetable — une exigence, pas une optimisation.

---

## 11. Coût RAM/CPU attendu et budget

### 11.1 Statut des chiffres

**Aucune mesure de coût de coordination n'a été effectuée.** Les valeurs ci-dessous sont des cibles de conception, exprimées pour être réfutées par COLLAB-1 et COLLAB-2. Elles ne doivent pas être citées comme performances Ravum. Machine de référence : 16 Go. Pendant la session de rédaction, ~2 Go étaient libres : les builds ont donc été limités (`CARGO_BUILD_JOBS=2`, tests sérialisés) et le prototype reste mono-processus sur des fixtures synthétiques.

### 11.2 Budget mémoire

| Poste | Cible | Hypothèse | Réfutation |
|---|---|---|---|
| Manifeste de snapshot | ~150–250 octets/fichier en mémoire | chemin + 32 o hash + mode + kind | 50 000 fichiers ⇒ 7,5–12,5 Mo ; à mesurer |
| Store de blobs | Partagé, dédupliqué | Les fichiers inchangés ne sont jamais recopiés | Si la déduplication échoue, le coût est celui du dépôt |
| Overlay par agent | < 50 Mo | Blobs modifiés + métadonnées | Un agent qui régénère un gros fichier généré explose le budget ⇒ refus |
| Journal des propositions | < 10 Mo / 10 000 propositions | Corps compressés, patch en diff | Un patch de 1 Mo × 10 000 = 10 Go ⇒ borne obligatoire par proposition |
| Index symbolique | Non chiffré | Dépend du langage et du compilateur | Probable poste dominant ; à mesurer |
| Coordinateur au repos | < 100 Mo RSS | Structures compactes, pas de cache de contenu | Un coordinateur gardant tous les snapshots échoue |

### 11.3 Budget CPU

| Opération | Cible indicative | Remarque |
|---|---|---|
| Hachage d'un fichier modifié | Linéaire en taille | SHA-256 ; débit dépendant du CPU et des extensions |
| Classification d'une proposition | < 20 ms p95 pour < 50 fichiers | Dominée par la résolution symbolique, pas le diff |
| Application d'un patch | < 5 ms p95 | Application exacte, pas de recherche de contexte |
| Publication (hors validation) | < 50 ms p95 | Écriture du manifeste + événement |
| Validation incrémentale | Dominée par le compilateur | Hors champ : coût du check, pas de la coordination |

La coordination doit être négligeable devant la compilation. Si la classification coûte plus de 10 % du temps de check, le modèle n'apporte pas le gain attendu et doit être rejeté (§14).

### 11.4 Ce qui ferait échouer le budget

Quatre signaux : manifeste > 1 Ko par fichier (structures trop riches, revoir la représentation) ; classification > 100 ms p95 sur 50 fichiers (résolution trop coûteuse, réduire la granularité) ; mémoire croissante sans borne (fuite de snapshots ou de propositions, corriger avant de continuer) ; rebases en cascade, plus de 3 par proposition en moyenne (le modèle ne converge pas à cette granularité).

---

## 12. Plan de tests obligatoires

Chaque scénario est exécutable, avec résultat attendu et critère d'échec explicite. Un test qui ne peut pas échouer n'est pas un test. Convention : `Txx` = test, `S` = scénario, `A` = critère d'échec.

**T01 — Fichiers indépendants.** `S` : A modifie `src/a.rs`, B modifie `src/b.rs`, aucune dépendance, même base. **Attendu** : deux `AUTO_SAFE`, publiées dans un ordre quelconque, les deux contenus présents, chaque publication attribuable. **A** : une modification absente ; un `CONFLICT` ; une publication non attribuable.

**T02 — Fonctions différentes du même fichier.** `S` : A modifie `fn alpha`, B modifie `fn beta`, hunks disjoints, aucun symbole partagé. **Attendu** : `REBASE_REVALIDATE` (jamais `AUTO_SAFE`), merge propre, deux corps présents, check ciblé sur les deux symboles. **A** : `AUTO_SAFE` ; un corps perdu ; check sur un seul symbole.

**T03 — Même fonction.** `S` : A et B modifient le corps de `fn alpha`, hunks chevauchants. **Attendu** : `CONFLICT` pour la seconde, aucune publication automatique, les deux versions inspectables, la première reste publiée. **A** : merge automatique ; un corps disparaît ; le fichier publié ne contient qu'une intention.

**T04 — Changement d'API utilisé par un autre.** `S` : A change la signature de `api::charge`, B ajoute un appel avec l'ancienne signature. **Attendu** : détection via le graphe de dépendances ; `REBASE_REVALIDATE` élargi, ou `CONFLICT` si le symbole n'est plus résoluble. Si les deux étaient publiés, le check doit échouer et la publication être refusée. **A** : publication d'un état qui ne compile pas ; publication d'un état qui compile mais dont l'appel a été silencieusement modifié ; `AUTO_SAFE` accordé.

**T05 — Snapshot périmé.** `S` : A observe `Sn`, B publie `Sn+1` modifiant un fichier ciblé par A, A soumet avec `base = Sn`. **Attendu** : `REBASE_REVALIDATE` ou `CONFLICT`, jamais d'application directe ; le diagnostic indique base et head. **A** : application directe ; merge sans revalidation ; message générique sans identités.

**T06 — Patch invalide.** `S` : préimage fausse ; patch malformé ; écriture hors `target_paths`. **Attendu** : `REJECT` avec motif structuré (`PATCH_APPLY`, `SCHEMA`, `OUT_OF_SCOPE`), aucune écriture, aucun blob orphelin référencé. **A** : application partielle ; fichier tronqué ; blob orphelin compté comme publié ; motif absent.

**T07 — Crash d'un agent.** `S` : SIGKILL avant soumission, pendant l'écriture du blob, après soumission avant réponse, pendant la validation. **Attendu** : proposition absente et aucun état partiel, ou présente avec état terminal ; snapshot canonique intact. **A** : proposition orpheline ; blob partiel référencé ; verrou non libéré bloquant les autres ; état canonique modifié.

**T08 — Commits simultanés.** `S` : deux publications tentent d'avancer la même ref au même moment. **Attendu** : une seule réussit ; l'autre échoue au CAS, est re-classifiée et retentée ou rejetée avec motif ; aucun commit orphelin, aucune ref perdue. **A** : les deux commits sont créés et l'un écrase l'autre ; la ref pointe sur un arbre qui ne correspond à aucun snapshot publié ; historique réécrit.

**T09 — Fichier généré / lockfile.** `S` : A modifie `Cargo.lock`, B modifie `package-lock.json`, C modifie un fichier marqué généré. **Attendu** : `REJECT(GENERATED_FILE)` avec instruction de régénérer ; si non déclaré, la politique de déclaration est appliquée et signalée. **A** : merge textuel d'un lockfile ; publication d'un lockfile incohérent avec le manifeste ; absence de diagnostic sur la voie de résolution.

**T10 — Retrait d'un contrat.** `S` : A supprime une postcondition publique ; B supprime une fonction publique utilisée ailleurs. **Attendu** : `MANUAL_REQUIRED` dans les deux cas, avec identification du contrat et des dépendants ; aucune publication automatique. **A** : publication automatique ; `AUTO_SAFE` ; contrat retiré sans trace ; dépendant cassé sans avertissement.

**T11 — Régression fonctionnelle après merge compilable.** `S` : A et B modifient des parties différentes d'un même module ; le merge est propre et compile ; le comportement change (ordre d'évaluation, seuil, condition aux limites). **Attendu** : le test de régression du module est exécuté parce que le module est impacté ; il échoue ; la proposition est `FAILED_VALIDATION` et n'est pas publiée ; le dernier état valide reste actif. **A** : publication parce que « ça compile » ; test non exécuté faute d'invalidation ; test marqué `SKIP` ; échec masqué par un `retry`.

**T12 — Intégration sans rebuild global.** `S` : modifier un fichier feuille sans dépendant ; compter les unités recompilées. **Attendu** : seules les unités impactées sont réévaluées ; les autres résultats sont réutilisés avec des clés identiques ; parité avec un build froid vérifiée sur les unités réévaluées. **A** : rebuild global ; réutilisation d'un résultat dont la clé a changé ; parité non vérifiée ; mesure absente.

**T13 — Refus sans écriture partielle.** `S` : provoquer un échec à chaque étape (schéma, CAS, merge, check, tests, publication, commit) et inspecter l'état. **Attendu** : aucun snapshot partiel, aucun blob référencé par un manifeste non publié, aucune ref avancée, aucun fichier canonique modifié ; le dernier état valide est intact (hash identique). **A** : un état partiel quelconque ; hash du dernier bon état modifié ; fichier temporaire laissé dans l'arbre.

**T14 — Changements d'un autre agent intacts.** `S` : après une série de publications et de rejets, comparer le snapshot courant à une reconstruction attendue. **Attendu** : toutes les propositions `PUBLISHED` sont présentes, aucune `REJECTED` n'a laissé de trace, aucune modification n'a été altérée par un merge ultérieur. **A** : une modification publiée disparaît ; une modification rejetée apparaît ; un contenu est modifié par un merge qui ne le ciblait pas.

**T15 — Preview reliée au snapshot exact.** `S` : provoquer une proposition invalide pendant qu'une preview est active ; comparer l'identité affichée au snapshot réellement exécuté. **Attendu** : la preview affiche son `snapshot_id` et son état de validation ; elle ne prétend pas correspondre au buffer courant ; l'ancienne version reste active et identifiée. **A** : la preview affiche le `snapshot_id` du buffer périmé ; identité absente ; version non validée exécutée ; l'utilisateur ne peut pas distinguer l'état affiché de l'état source.

### 12.1 Matrice de couverture

| Invariant | Tests |
|---|---|
| SW-01 | T01, T13 |
| SW-02 | T07, T13, T14 |
| SW-03 | T08, T13 |
| SW-04 | T05, T06, T09 |
| SW-05 | T06, T09, T10, T13 |
| SW-06 | T01, T08, T14 |
| SW-07 | T11, T15 |
| SW-08 | T08, T14 |
| SW-09 | T05, T06 (rejeu) |
| SW-10 | T05 (cascade), T07 |

Un invariant sans test est un invariant non tenu. La matrice doit rester complète à chaque évolution.

---

## 13. Étapes COLLAB-0 → COLLAB-4

Ordonnées par dépendance et critère de sortie, pas par durée. Aucune étape n'est terminée sur une démonstration partielle.

### COLLAB-0 — Formalisation et fixtures

**Objectif** : rendre cette RFC falsifiable avant d'écrire du code de coordination. **Livrables** : schémas versionnés (`ChangeProposal`, `Snapshot`, `MergeClass`, `PublicationEvent`) ; fixtures synthétiques (4 modules, 20 fichiers, dépendances connues, un généré, un lockfile, un contrat) ; les 15 scénarios écrits en **échec** (red) avec leurs critères ; un oracle « état attendu » indépendant de l'implémentation. **Sortie** : les 15 tests existent et échouent pour la bonne raison (pas une erreur du test) ; chaque SW a au moins un test ; les schémas sont relus par une personne distincte ; aucune mesure de performance revendiquée.

### COLLAB-1 — Noyau mono-processus

**Objectif** : démontrer le modèle sans Git, sans multi-processus, sans compilateur. **Périmètre** : store content-addressed local ; snapshots et overlays ; proposition et validation de schéma ; coordinateur mono-thread avec journal append-only ; classification à cinq niveaux ; écriture canonique **désactivée** (tout en mémoire ou en répertoire de test). **Sortie** : les 15 tests passent, négatifs compris ; le rejeu d'un journal dans un ordre différent donne les mêmes décisions (SW-09) ; aucun état partiel après injection de faute (T13) ; budget mémoire mesuré et documenté, sans build lourd.

### COLLAB-2 — Multi-processus, robustesse

**Objectif** : résister à la concurrence réelle et aux crashs. **Périmètre** : plusieurs processus agents, un coordinateur ; écriture concurrente dans le store (déduplication sous course) ; files bornées, équité, escalade ; crash recovery (journal, reprise, idempotence) ; aucune écriture Git. **Sortie** : T07 et la publication concurrente simulée passent sur 100 itérations sans état orphelin ; aucune famine sur charge mixte soutenue (attente p99 sous seuil déclaré) ; rebases par proposition bornés et gel fonctionnel ; fuites mémoire mesurées et bornées sur exécution longue.

### COLLAB-3 — Écrivain Git unique

**Objectif** : démontrer l'intégration Git sans conflit d'index ni réécriture. **Périmètre** : `mktree` + `commit-tree` + `update-ref` CAS ; `GIT_INDEX_FILE` dédié ; trailers ; reprise après échec CAS ; une branche d'intégration, pas de branche par agent. **Sortie** : T08 passe avec des publications réellement concurrentes ; chaque commit est rattachable à une proposition et à une base ; aucun `reset`/`rebase`/`merge`/`checkout` de haut niveau exécuté par le coordinateur (audit des commandes) ; un revert restaure exactement un snapshot antérieur sans réécrire l'historique.

### COLLAB-4 — Intégration compilateur et preview

**Objectif** : démontrer que la coordination ne dégrade ni l'invalidation ni la sûreté d'activation. **Périmètre** : connexion au compilateur incrémental (ou adaptateur Rust) ; clés incluant le snapshot ; rejet des `STALE` ; preview affichant son `snapshot_id` ; aucune activation non validée. **Sortie** : T11, T12, T15 passent ; parité froid/chaud vérifiée sur les unités réévaluées après merge ; un résultat produit sur un snapshot non ancêtre ne peut jamais être publié ; coût de coordination < 10 % du temps de check, sinon le modèle est rejeté.

---

## 14. Conditions d'acceptation et raisons de rejeter

### 14.1 Conditions d'acceptation

| # | Condition | Preuve exigée |
|---|---|---|
| AC-01 | Les 15 scénarios passent, négatifs compris | Rapport d'exécution, SHA, environnement |
| AC-02 | Chaque SW a un test qui échoue si l'invariant est violé | Mutation testing ou test négatif explicite |
| AC-03 | Aucun état partiel après injection de faute à chaque étape | Rapport T13 avec liste des étapes |
| AC-04 | Coût de coordination < 10 % du temps de check sur la fixture | Mesure, pas estimation |
| AC-05 | Budget mémoire respecté sur la fixture, marge documentée | Mesure RSS |
| AC-06 | Temps total agent jusqu'à livraison vérifiée réduit vs baseline worktrees | Mesure S8 ou équivalent, mêmes conditions |
| AC-07 | Aucun commit produit par un autre chemin que l'écrivain unique | Audit des commandes et permissions |
| AC-08 | Un audit externe reconstruit proposition → commit sans base volatile | Reconstruction à l'aveugle |
| AC-09 | Une personne distincte de l'auteur a relu et attaqué le modèle | Rapport de revue |
| AC-10 | RFC ratifiée par le mainteneur, avec un ADR distinct | Décision humaine datée |

AC-06 est décisif : une coordination correcte mais sans gain de temps total ne justifie pas son coût de maintenance. La correction seule n'est pas un argument suffisant — les worktrees sont déjà corrects, simplement coûteux.

### 14.2 Raisons de rejeter

| # | Situation | Pourquoi |
|---|---|---|
| R-01 | Taux de conflit réel < 5 % des propositions | Le coût dépasse le bénéfice ; les worktrees suffisent |
| R-02 | Agents sur sous-systèmes disjoints avec propriétaires | La règle de la consolidation est déjà optimale ; le coordinateur devient un point unique de défaillance |
| R-03 | Équipe petite (1–2 agents) sérialisant naturellement | Aucun gain mesurable, coût de maintenance réel |
| R-04 | Résolution symbolique non fiable sur la stack cible | Granularité retombée au fichier, `AUTO_SAFE` douteux, valeur perdue |
| R-05 | Coût de coordination > 10 % du temps de check | P0-A dégradé pour un bénéfice organisationnel |
| R-06 | Budget mémoire intenable sur 16 Go avec les caches du compilateur | P0-H violé |
| R-07 | Aucun gain mesuré sur le temps total (AC-06 échoue) | Le modèle n'apporte pas ce qu'il promet |
| R-08 | Le modèle exige de réécrire l'historique ou de contourner les protections | Incompatible avec ADR-031 et ADR-041 |
| R-09 | Un cas de perte silencieuse subsiste et n'est pas détectable | Violation de SW-02, non négociable |
| R-10 | Complexité de maintenance au-delà de la capacité de l'équipe | INV-10 (budget de complexité) |

### 14.3 Quand préférer worktrees et branches

**Par défaut, les worktrees restent la bonne réponse** : simples, éprouvés, sans point unique de défaillance, et ils isolent réellement les agents. Le modèle de cette RFC n'a de sens que si trois conditions sont réunies :

1. **Volume** : plusieurs agents écrivent simultanément dans le même périmètre, assez souvent pour que duplication et intégration coûtent cher.
2. **Proximité** : les changements se chevauchent ou se touchent assez pour que le merge soit fréquent — sinon l'isolation par worktree est meilleure.
3. **Capacité de vérification** : le compilateur fournit une identité de snapshot et un index symbolique fiables, sinon la classification est décorative.

Si l'une manque, la réponse honnête est : garder les worktrees, améliorer l'outillage de merge et de test, ne pas construire de coordinateur.

---

## 15. Limites explicites du MVP et ce qui n'est PAS démontré

### 15.1 Périmètre

| Dimension | Inclus | Exclu |
|---|---|---|
| Dépôt | Un dépôt, une racine | Sous-modules, monorepo multi-racines, dépôts liés |
| Branche | Une branche d'intégration | Multi-branches simultanées, feature branch par agent |
| Fichiers | Texte UTF-8, fins de ligne normalisées | Binaires, encodages exotiques, fichiers volumineux |
| Langage | Un langage à la fois (Rust en MVP-0a) | Multi-langage dans une même proposition |
| Symboles | Résolution via adaptateur (rust-analyzer) | Résolution native Ravum, macros complexes, génériques avancés |
| Contrats | Déclaration et classification | Vérification sémantique des obligations |
| Fusion | Texte + symboles | AST-aware, typed patching, résolution automatique de conflits |
| Distribution | Mono-machine | Multi-machine, réseau, CRDT |
| Git | Plumbing local | Push, PR automatique, protections distantes avancées |
| Activation | Aucune (hors périmètre) | Hot swap, preview runtime |
| Preuves | Aucune | Vérification formelle des merges |

### 15.2 Ce qui n'est PAS démontré

1. **Le prototype COLLAB-1 existe mais reste étroit.** `crates/ravum-workspace-coordinator` implémente : propositions contre un snapshot de base, préconditions de hash, chevauchement sur les lignes réellement modifiées et les symboles déclarés, décisions `AUTO_SAFE` / `REBASE_REVALIDATE` / `CONFLICT` / `REJECT` / `MANUAL_REQUIRED`, journal append-only, overlay en mémoire, écrivain Git unique. Il ne fournit **ni** rebase automatique, **ni** fusion AST, **ni** CRDT. Détail et limites : `evidence/multi-agent-live/README.md`.
2. **Aucune mesure de coût de coordination.** Les cibles de §11 ne sont pas des performances et n'ont pas été observées. Le prototype n'a pas été instrumenté en mémoire/CPU.
3. **La correction sémantique après merge n'est pas prouvée.** Le modèle réduit le risque de perte silencieuse ; il ne l'élimine pas. T11 est un risque assumé, détecté quand le test existe, invisible sinon.
4. **La préservation des contrats n'est pas démontrée.** Sur Rust et TypeScript, il n'y a pas de contrats natifs : la classification sur contrat est une politique, pas une preuve.
5. **La fiabilité de la résolution symbolique n'est pas établie.** Elle dépend de l'adaptateur et de ses trous (macros, traits, code généré), qui doivent produire `UNKNOWN` — et `UNKNOWN` ne vaut pas succès.
6. **Le gain de temps total n'est pas mesuré.** C'est pourtant le critère décisif (AC-06). Sans lui, la RFC reste une hypothèse.
7. **Le comportement à l'échelle est inconnu.** 20 fichiers de fixture ne disent rien de 50 000 ; la résistance à un agent adversarial qui ment sur `impacted_contracts` ou `declared_dependencies` n'est pas démontrée non plus — la détection est partielle.
8. **L'absence de famine n'est pas prouvée.** Argumentée par construction (pas de verrous agents), non mesurée sous charge réelle.
9. **La compatibilité avec le compilateur incrémental est théorique.** Les clés proposées sont cohérentes avec ADR-012, mais aucun test de parité froid/chaud n'a été exécuté.

### 15.3 Risques résiduels

| Risque | Probabilité | Impact | Mitigation |
|---|---|---|---|
| Faux `AUTO_SAFE` sur un changement d'interface | Moyenne | Élevé | Invalidation conservatrice ; audit des classes |
| Rebase en cascade | Moyenne | Moyen | Gel après N tentatives, escalade |
| Coût symbolique dominant | Élevée | Moyen | Retour à la granularité fichier, assumé |
| Point unique de défaillance (coordinateur) | Certaine | Moyen | Journal append-only, reprise, retour possible aux worktrees |
| Dérive de la politique de contrats | Faible | Élevé | `MANUAL_REQUIRED` par défaut, revue humaine |
| Complexité de maintenance | Élevée | Élevé | Étapes bornées, critère R-10 |

---

## Annexe A — Glossaire

| Terme | Définition |
|---|---|
| Snapshot | Manifeste immuable associant chemins et blobs, identifié par `SourceSnapshotId` |
| Overlay | Blobs nouveaux produits par un agent, non publiés |
| ChangeProposal | Unité d'intention : base, cibles, préconditions, patch |
| Publication | Écriture d'un nouveau snapshot canonique après validation |
| `STALE` | Résultat produit sur un état qui n'est plus un ancêtre de l'état courant |
| Fail-closed | En cas d'incertitude, refus et conservation du dernier état valide |

## Annexe B — Journal des versions

| Date | Version | Changement |
|---|---|---|
| 2026-11-08 | 0.1 | Proposition initiale. Aucun prototype, aucune mesure, aucun numéro ADR |
| 2026-11-08 | 0.2 | Prototype COLLAB-1 implémenté et exécuté (`crates/ravum-workspace-coordinator`) : décisions, journal, écrivain Git unique, expérience réelle à trois agents. Toujours `PROPOSED`, aucun numéro ADR |

## Annexe C — À lire avant de contester

1. `docs/architecture/PROJECT_INVARIANTS.md` — P0 et INV qui bornent les choix.
2. `docs/specs/03-compilation-continue-hot-swap.md` — états de patch et invariants d'activation.
3. `docs/adr/ADR-021` et `ADR-022` — activation atomique, limites du rollback.
4. `docs/adr/ADR-012` — clés de requête et invalidation.
5. `docs/adr/ADR-008` — concurrence, annulation, backpressure.
6. `docs/adr/ADR-027` — l'API machine qui porterait ce protocole.
7. `docs/reviews/2026-11-08-consolidation.md` §8.1 — la règle conservatrice actuelle, non abrogée.

Un désaccord formulé sans avoir lu ces documents est `NOT_REVIEWED`, au sens de INV-13.
