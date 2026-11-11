# MVP-0a — architecture réellement implémentée (M1)

> **Statut : expérimental, en cours de qualification.** Ce document décrit le code
> qui existe dans ce dépôt à la date indiquée en fin de fichier. Il ne décrit pas
> d'intention future, pas de garantie de production et pas un ABI publié.
> Les ADR restent `PROPOSED` ; rien ici ne les ratifie.

## 1. Ce qui est démontré, et ce qui ne l'est pas

Démontré par des tests exécutés (voir `docs/mvp/M1-evidence.md`) :

| Capacité | Preuve |
|---|---|
| Un processus vivant change de comportement après compilation d'un patch compatible | test `compatible_patch_changes_behaviour_without_restart` + démonstration `scripts/demo-m1.ps1` |
| Un module d'ABI native incompatible est refusé avant tout appel | test `incompatible_native_abi_is_refused` |
| Un module à signature logique incompatible est refusé | test `incompatible_logical_signature_is_refused` |
| Un export manquant est refusé | test `missing_export_is_refused` |
| Une source invalide n'est jamais compilée ni activée | test `invalid_source_is_refused_before_any_build` |
| Un résultat produit sur un snapshot périmé est refusé | tests `stale_build_is_refused`, `result_built_from_a_superseded_snapshot_is_never_activated` |
| Un appel en vol empêche la libération du module qu'il exécute | test `in_flight_call_keeps_the_old_module_loaded` |
| Aucune activation sans verdict de vérification `tested` | test `activation_without_evidence_is_refused` |
| Deux agents intègrent deux fonctions du même fichier, puis le résultat fusionné est activé | test `two_agents_integrate_into_one_file_and_the_result_hot_activates` |
| Un conflit est refusé sans altérer le travail déjà intégré | tests `conflicting_intent_is_refused_and_the_other_agent_work_survives`, `same_symbol_twice_is_a_conflict_and_nothing_is_lost` |
| L'écrivain Git unique ne commite que les chemins intégrés | test `commits_only_the_integrated_paths_with_attribution` |

Non démontré, et **non revendiqué** :

- migration d'état entre versions (ADR-022 ouvert) ;
- reprise après panique dans un patch : `-C panic=abort` fait avorter le processus
  hôte, il n'y a pas d'isolation de faute ;
- passage de structures, d'objets ou de références à travers l'ABI de patch ;
- comportement temps réel : le thread de démonstration n'est pas un callback audio ;
- fusion sémantique, AST ou CRDT ;
- preuve formelle de quoi que ce soit : le seul statut produit est `tested`.

## 2. Composants et responsabilités

```text
ravum-protocol                 identités, diagnostics, patch/ABI, preuves, propositions
      ▲        ▲          ▲
      │        │          │
ravum-snapshot  ravum-rust-adapter  ravum-patch-host
 (immuable,      (rustc CLI public,   (ABI vérifiée, activation
  content-        diagnostics          atomique, appels en vol,
  addressed)      structurés)          reclamation différée)
      ▲                ▲                     ▲
      └────────┬───────┴─────────────────────┘
               │
        ravum-coordinator      (ordre des étapes, STALE, contrat smoke, file coalescente)
               │
      ┌────────┴─────────┐
 ravum-cli           apps/ravum-demo-host
 (check/build/patch   (application vivante, IPC JSON sur stdio)
  /run/watch/status
  /bench)
```

Composant séparé, expérimental :

```text
ravum-workspace-coordinator   propositions multi-agents (COLLAB-1) + écrivain Git unique
```

### 2.1 `ravum-protocol`

Contrat machine versionné (`PATCH_SCHEMA_VERSION = 1`). Aucune dépendance à une API
privée de `rustc` (INV-11). Cinq identités **distinctes au niveau du type** :

| Identité | Calcul | Consommateur réel |
|---|---|---|
| `SourceSnapshotId` | SHA-256 canonique du dictionnaire chemin→contenu | refus `STALE`, preuves, journal |
| `BuildContextId` | SHA-256 de (version rustc, cible, édition, flags) | preuves, futur cache d'artefacts |
| `ArtifactId` | SHA-256 des octets de l'artefact | anti-réutilisation, détection d'altération |
| `VerificationKey` | SHA-256 de (propriété, hypothèses, outil, version) | preuves, politique d'activation |
| `ActiveVersionId` | compteur monotone du `PatchHost` | statut hôte, rapports d'activation |

### 2.2 `ravum-snapshot`

Snapshot immuable et adressé par contenu. `with_overlay` produit un **nouveau**
snapshot ; l'ancien n'est jamais modifié. Canonicalisation : séparateurs `/`,
chemins relatifs, tri lexicographique, longueur + contenu dans le digest.

Conséquence testable : comparer un identifiant est une opération suffisante pour
savoir qu'un résultat est périmé.

### 2.3 `ravum-rust-adapter`

Deux opérations, uniquement via la CLI publique de `rustc` :

- `check` : `--emit=metadata --error-format=json --crate-type lib` (analyse seule) ;
- `build_cdylib` : `--crate-type cdylib -C opt-level=1 -C debuginfo=0 -C panic=abort`.

`RUSTFLAGS`, `RUSTC_WRAPPER` et `RUSTC_WORKSPACE_WRAPPER` sont retirés de
l'environnement de l'enfant : un résultat doit être attribuable au contexte déclaré.
Les diagnostics JSON de `rustc` sont convertis en `DiagnosticSet` lié au snapshot.

`rustc` réalise la génération de code **et** l'édition de liens dans un seul
processus : `build_ms` couvre les deux et aucun découpage n'est inventé.

### 2.4 `ravum-patch-host`

Chargement et validation d'un module natif :

1. `ravum_abi_version()` — porte rapide, refus immédiat si différente de l'attendu ;
2. `ravum_manifest_v1()` — JSON embarqué (identifiant de patch, export, signature) ;
3. résolution de `ravum_compute` — seulement après les deux contrôles précédents ;
4. `ArtifactId` recalculé sur les octets réellement chargés.

Publication : `RwLock<Arc<LoadedModule>>`. Un appel clone l'`Arc`, **relâche le
verrou**, puis appelle : un appel long ne bloque donc ni la lecture de la version
active, ni une activation.

Libération : `DeferredLibrary::drop` n'appelle jamais `FreeLibrary`/`dlclose` sur le
thread appelant ; l'handle part vers un thread `ravum-reclaimer` dédié. C'est un
prérequis de M3 (interdiction de libérer sur le callback audio), pas une preuve RT.

### 2.5 `ravum-coordinator`

Ordre imposé : `check` → `build` → `smoke_verify` → **re-vérification de fraîcheur**
→ publication. La fraîcheur est contrôlée *après* les étapes coûteuses, parce que
c'est précisément là qu'un résultat peut devenir périmé (INV-07).

`WorkspaceSession` fournit un verdict explicite `Delivered` / `Stale { job, current }`
et compte les refus. `CoalescingQueue` ne conserve que la dernière édition en attente.

Contrat `smoke` : chargement, valeurs finies sur 5 entrées fixes, déterminisme sur
deux appels consécutifs. Statut produit : `tested`. Portée explicitement limitée
dans `EvidenceRecord::assumptions`.

### 2.6 `ravum-cli`

| Commande | Réellement implémentée | Limite |
|---|---|---|
| `check <fichier>` | oui, `--json` | un seul fichier d'unité de patch |
| `build <fichier>` | oui, `--json` | refuse si l'analyse a des erreurs |
| `patch <artefact>` | oui | valide ABI/manifeste/symboles hors application vivante |
| `run <fichier>` | oui | démonstration, pas un serveur |
| `watch <fichier>` | oui | surveillance par scrutation (pas de notifications FS) |
| `status` | oui | lit `target/ravum/state.json` |
| `bench <a> <b>` | oui | mesures locales, non portables |

### 2.7 `apps/ravum-demo-host`

Processus de longue durée, IPC JSON par lignes sur stdin/stdout. Il ne redémarre
jamais ; il publie les ticks calculés par le module actif et accepte des commandes
`apply`. Chaque `apply` refait la validation complète côté hôte : le CLI n'est pas
cru sur parole.

### 2.8 `ravum-workspace-coordinator` (COLLAB-1)
- `ChangeProposal` : base explicite, chemins et symboles cibles, hashes attendus,
  dépendances déclarées, contrats impactés, diff unifié.
- Décisions : `AUTO_SAFE`, `REBASE_REVALIDATE`, `CONFLICT`, `REJECT`,
  `MANUAL_REQUIRED`, chacune avec ses raisons conservées.
- Intégration en overlay mémoire, appliquée séquentiellement ; un refus ne modifie
  rien et n'efface jamais une intention déjà intégrée.
- `GitWriter` : seul composant autorisé à exécuter des commandes Git mutantes.
  `git commit --only -- <chemins>` évite d'embarquer le travail d'un autre agent ;
  le parent du nouveau commit est comparé au HEAD observé pour détecter un écrivain
  concurrent.

Application de patch : hunks exacts, sans correspondance floue. Une application
« tolérante » est précisément le mécanisme qui transforme un conflit sémantique en
code corrompu.

### 2.9 `ravum-proof` (M2)

- `Obligation` : un claim (`finite`, `deterministic`, `non_negative`, `equals`,
  `monotonic_non_decreasing`) sur un **domaine fini explicite**.
- `verification_key` : SHA-256 de (identifiant, claim, entrées en bits, hypothèses
  triées, ABI, snapshot, contexte de build, artefact, outil, version, schéma).
  Toute mutation change la clé — c'est ce qui rend l'invalidation mécanique.
- `ExhaustiveVerifier` : énumère le domaine sur l'artefact réel ; ne renvoie `Proved`
  que si tout le domaine est couvert **dans le budget de temps**.
- `KaniVerifier` : détecte un prouveur externe et refuse de conclure sans harness.
- `EvidenceLedger` : une ligne JSON par verdict, écrite et flushée immédiatement ;
  lecture *fail-closed* (une ligne corrompue invalide le ledger).
- `ProofBroker` : `Fresh` seulement si la clé, le contexte et l'outil correspondent ;
  sinon re-exécution. Un échec est mis en cache : un contre-exemple connu n'est pas
  re-testé et ne devient jamais un succès.
- Gate : `Pipeline::with_obligations` refuse l'activation si une obligation n'est pas
  satisfaite, et **erreur** si un broker est absent. Rien n'est sauté silencieusement.

## 3. Flux complet d'une itération
```text
fichier modifié
   │
   ├─ watcher (thread) : nouveau contenu → nouveau SnapshotId → session.supersede()
   │
   ├─ check (rustc --emit=metadata)        → DiagnosticSet lié au snapshot
   │      erreurs ? → REFUS, rien n'est compilé
   ├─ build (rustc --crate-type=cdylib)    → ArtifactId + BuildContextId
   ├─ smoke contract (chargement réel)     → EvidenceRecord{status: tested}
   ├─ fraîcheur : snapshot du build == snapshot courant ?
   │      non ? → STALE, rien n'est activé
   └─ apply (IPC) → PatchHost
          ABI ? manifeste ? signature ? preuve ? déjà actif ?
             refus → version précédente intacte, raison journalisée
             accord → swap atomique de l'Arc, ancienne version libérée
                       seulement quand plus aucun appel ne l'exécute
```

## 4. Budget de ressources

- builds `rustc` séquentiels dans les tests (`--test-threads=1`) : la machine de
  référence a 16 Go et peu de marge libre pendant la session ;
- compilation des tests en `CARGO_BUILD_JOBS=2` ;
- aucun daemon persistant : `watch` est un processus au premier plan ;
- un seul thread de surveillance + un thread de reclamation par processus.

## 5. Ce que ce MVP ne remplace pas

Le MVP ne constitue ni un compilateur Ravum, ni une IR, ni un vérificateur. Il
démontre une **boucle** sur du Rust existant, avec des frontières explicites, dans
l'optique de la stratégie F. La décision M5 (langage autonome) n'est pas prise.
