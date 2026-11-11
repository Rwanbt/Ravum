# MVP-0c / L3 — Collaboration multi-agents : statuts sémantiques versionnés et oracle injectable

Statut : **implémenté, testé, mesuré localement** (branche `feat/mvp0c-hardening`,
worktree non commité — le Lead est seul écrivain Git). Ce lot durcit le
coordinateur multi-agents sans casser COLLAB-1→4.

Preuves de cette page : commandes exécutées sur la machine de référence
(Windows, 16 Go de RAM, plusieurs workers en parallèle, `CARGO_BUILD_JOBS=2`,
campagnes `cargo test -- --test-threads=1`), `rustc 1.95.0 (59807616e
2026-04-14)`, `cargo 1.95.0`, `git 2.51.1.windows.1`. Les chiffres exacts sont
dans `evidence/mvp0c-collab-2026-11-09.json`.

---

## 1. Objectif et non-régression

COLLAB-1→4 parlaient une seule langue : une `DecisionKind` (`AUTO_SAFE`,
`CONFLICT`, `REJECT`, `MANUAL_REQUIRED`, `REBASE_REVALIDATE`) plus des raisons.
Deux problèmes :

1. `AUTO_SAFE` ne disait pas **jusqu'où** la fusion avait été vérifiée ; un
   patch textuellement applicable pouvait être lu comme une acceptation
   sémantique ;
2. aucune place stable pour une connaissance sémantique externe (analyse
   inter-crates L1) dans la décision.

Ce lot ajoute :

* un **axe d'évaluation versionné et additif** (`EvaluationLevel`) écrit dans le
  journal et dans les enregistrements, sans renommer ni retirer les décisions
  existantes ;
* un **trait `SemanticOracle` injectable** (faux oracle en test, branchement L1
  réel ensuite côté Lead) ;
* la **correction du chemin** : sous le protocole v2, aucun `AUTO_SAFE` n'est
  émis sur la seule base d'un patch textuellement applicable ;
* un corpus négatif `fixtures/collab5` couvrant les 10 cas du mandat avec
  décision + niveau + raison journalisée.

**Non-régression** : les 87 tests existants du crate sont conservés et verts
(aucun test supprimé, modifié ou affaibli), le journal v1 reste lisible, et
`cargo build -p ravum-cli` compile sans modification de `ravum-cli` (l'API est
purement additive ; `AcceptedChange.evaluation` est optionnel).

---

## 2. Protocole versionné

### 2.1 Constantes et modes

`COORDINATOR_SCHEMA_VERSION = 2` (`crates/ravum-workspace-coordinator/src/evaluation.rs`).

| mode | construction | signification d'`AUTO_SAFE` |
|---|---|---|
| `V1_COLLAB_COMPAT` (défaut historique) | `Coordinator::new(base)` | « texte applicable » uniquement. Le record porte `protocol=V1_COLLAB_COMPAT` et `level=TEXTUALLY_APPLICABLE` : aucun consommateur ne peut le confondre avec une acceptation sémantique. |
| `V2_SEMANTIC_STATUSES` | `Coordinator::v2(base)` ou `.with_evaluation_protocol(V2SemanticStatuses)` | au moins `SEMANTICALLY_CHECKED` (ou `TESTED`/`PROVED`), justifié par un `MergeVerifier` **déclaré** et un oracle non bloquant. |

API additive ajoutée au coordinateur :

```rust
Coordinator::v2(base)                       // protocole strict
.with_evaluation_protocol(protocol)         // v1 par défaut, v2 explicite
.with_semantic_oracle(Arc<dyn SemanticOracle>)  // oracle injecté (défaut : textuel)
.protocol()                                 // EvaluationProtocol
.oracle_name()                              // oracle injecté, si déclaré
.evaluations()                              // &[EvaluationRecord], une par décision
.evaluation_for(proposal_id)                // dernier record pour une proposition
```

Le binaire `ravum-collab` expose `--protocol v1|v2` (v1 par défaut), et son
rapport `--json` contient `protocol`, `coordinator_schema_version`, `oracle`,
`evaluations` en plus des champs existants.

### 2.2 Journal : v2 rétro-compatible

Une ligne v1 était un `JournalEntry` nu (`seq`, `decision`, `snapshot_before`,
`snapshot_after`). Une ligne v2 est un **sur-ensemble** de ce même objet :

```json
{
  "seq": 2,
  "decision": { "...": "...", "schema_version": 1 },
  "snapshot_before": "sha256…",
  "snapshot_after": "sha256…",
  "schema_version": 2,
  "evaluation": {
    "schema_version": 2,
    "protocol": "V2_SEMANTIC_STATUSES",
    "level": "SEMANTICALLY_CHECKED",
    "decision": "AUTO_SAFE",
    "oracle": { "oracle": "textual-semantics-v1", "verdict": "clear", "findings": [], "evidence_id": "…" },
    "verification": { "status": "passed", "checks": [ { "kind": "metadata_check", "status": "passed", "evidence_id": "…" } ], "evidence_id": "…" },
    "reasons": ["…"],
    "evidence_ids": ["…"],
    "journal_seq": 2
  }
}
```

* un lecteur v1 `serde_json::from_str::<JournalEntry>(ligne)` continue de parser
  la ligne (les champs inconnus sont ignorés) : c'est ce que fait
  `tests/journal_durability.rs`, resté vert ;
* le lecteur tolérant `read_journal_records(path)` renvoie
  `CoordinatorJournalRecord { schema_version (1 pour un fichier v1), entry, evaluation: Option<…> }` ;
  une ligne v1 est lue **sans perte** (`schema_version = 1`, `evaluation = None`,
  `entry` identique) ;
* une ligne v2 sans `evaluation` est refusée (pas de v2 incomplet silencieux).

Tests : `evaluation::tests::a_v1_journal_line_is_read_without_loss`,
`a_v2_journal_line_round_trips_with_its_evaluation`,
`a_v2_line_without_evaluation_is_refused`,
`coordinator::tests::v2_journal_lines_are_v2_records_and_read_back_without_loss`,
et `tests/collab5_semantic_statuses::a_v1_journal_file_is_read_without_data_loss`
(vrai fichier v1 sur disque).

---

## 3. Statuts d'évaluation

`EvaluationLevel` (`evaluation.rs`), ordonnés du plus faible au plus fort :

| niveau | atteint quand | intègre ? |
|---|---|---|
| `TEXTUALLY_APPLICABLE` | patch exact, préconditions textuelles OK, **aucune vérification déclarée** (ou vérification indisponible/incomplète) | **non** : la décision est `MANUAL_REQUIRED`, ou `REBASE_REVALIDATE` si la seule alerte bloquante est une rupture d'interface utilisée ailleurs |
| `SEMANTICALLY_CHECKED` | vérificateur déclaré : tous les checks requis passent sur le snapshot candidat exact | oui (`AUTO_SAFE`) |
| `TESTED` | idem + le harnais de test a été **compilé et exécuté** avec succès | oui |
| `PROVED` | idem + `BoundedProof` explicite (propriété, périmètre, hypothèses, outil, version, snapshot cible, evidence id) applicable au snapshot candidat | oui, **borné uniquement** |
| `REJECTED` | `REJECT` ou `CONFLICT` | non |
| `MANUAL_REQUIRED` | arbitrage humain requis (politique, oracle bloquant, vérification impossible) | non |

Règles dures codées et testées :

* `AUTO_SAFE` sans record de vérification `passed` ⇒
  `level = TEXTUALLY_APPLICABLE` ; sous v2 cette combinaison n'est **jamais**
  produite (le gate refuse) ; sous v1 elle est marquée
  `is_legacy_textual_auto_safe() = true` ;
* un « verifier » qui prétend `Passed` en omettant un check requis est refusé
  (`MANUAL_REQUIRED`) ;
* `PROVED` sans provenance complète est ignoré (le niveau retombe à
  `SEMANTICALLY_CHECKED`/`TESTED`) et la raison “bounded proof ignored” est
  journalisée ;
* `PROVED` n'est jamais implicite : aucune décision ne peut le déduire d'un
  oracle « clear » sans `BoundedProof`.

---

## 4. Invariant central : aucun `AUTO_SAFE` sur la seule applicabilité textuelle

Énoncé vérifié : **un patch textuellement applicable sans vérification déclarée
ne produit jamais `AUTO_SAFE` sous le protocole v2 ; il produit
`MANUAL_REQUIRED` (ou `REBASE_REVALIDATE` pour une rupture d'interface).**

Preuves exécutées (toutes vertes) :

1. `coordinator::tests::v2_textually_applicable_patch_without_declared_verification_is_never_auto_safe`
   — patch réel, `Coordinator::v2` sans vérificateur : décision
   `MANUAL_REQUIRED`, snapshot inchangé, record d'évaluation
   `TEXTUALLY_APPLICABLE`/`V2_SEMANTIC_STATUSES`, raison « textual
   applicability alone ».
2. `tests/collab5_semantic_statuses::a_textually_applicable_patch_without_declared_verification_is_never_auto_safe`
   — même invariant sur l'API publique du crate, plus le témoin v1 : le même
   patch sous `Coordinator::new` est `AUTO_SAFE` mais marqué legacy.
3. `tests/collab5_semantic_statuses::the_corpus_contains_the_ten_mandate_cases`
   — le corpus refuse tout cas sans vérificateur qui attendrait `AUTO_SAFE`.
4. `tests/collab5_semantic_statuses::the_ten_cases_produce_their_expected_statuses_and_reasons`
   — boucle sur les 10 cas : tout `AUTO_SAFE` observé exige un vérificateur
   déclaré, un niveau intègre et un record de vérification `passed`.
5. Démonstration CLI réelle (voir §7.3) : `--protocol v2` sans `--verify` sur un
   patch qui s'applique exactement ⇒ `MANUAL_REQUIRED` ×2 /
   `TEXTUALLY_APPLICABLE` ; avec `--verify` (rustc réel) ⇒ `AUTO_SAFE`
   `SEMANTICALLY_CHECKED` + `CONFLICT`.

La branche v1 reste une **compatibilité explicite** : c'est ce qui préserve les
appelants existants (`ravum-cli` `collab_cycle`, démos COLLAB-1) et les 87 tests.
Elle est signalée dans chaque record (`V1_COLLAB_COMPAT` + `TEXTUALLY_APPLICABLE`
+ `is_legacy_textual_auto_safe`).

---

## 5. `SemanticOracle` : interface stable, branchement L1 différé

`oracle.rs` définit le point d'injection ; le coordinateur ne dépend que du
trait (aucun import de `ravum-daemon`, `ravum-unit-graph` ou d'un analyseur L1).

```rust
pub trait SemanticOracle: Send + Sync {
    fn name(&self) -> &'static str;
    fn evaluate(&self, request: &OracleRequest) -> OracleVerdict;
}

pub struct OracleRequest {
    proposal_id, agent_id, snapshot_before, snapshot_after,
    changed_paths, declared_symbols, declared_dependencies,
    impacted_contracts, unified_patch, required_checks,
}

pub enum OracleVerdict {
    Clear { bounded_proof: Option<BoundedProof> },
    Findings(Vec<OracleFinding>),
    Unavailable { reason: String },
}
```

* `OracleFindingKind` : `INTERFACE_BREAK`, `UNDECLARED_DEPENDENCY`,
  `WEAKENED_CONTRACT`, `ABI_CHANGE`, `AMBIGUOUS_IMPORT`, `LOCKFILE_CHANGE`,
  `FUNCTIONAL_REGRESSION`, `STALE_SNAPSHOT`, `OTHER` ;
* `OracleSeverity` : `info` / `warning` / `blocking` — seuls les `blocking`
  refusent l'intégration ;
* règle de décision : une alerte bloquante **autre** qu'une rupture d'interface
  ⇒ `MANUAL_REQUIRED` ; si **seules** des ruptures d'interface sont bloquantes
  ⇒ `REBASE_REVALIDATE` (« au minimum revalidé », jamais intégré) ;
* `Unavailable` ⇒ `MANUAL_REQUIRED` (fail-closed) ;
* `Clear` sans vérificateur déclaré ⇒ `MANUAL_REQUIRED` ;
* les warnings sont journalisés mais ne remplacent jamais une vérification ;
* côté coordinateur : le coordinateur ajoute lui-même le finding
  `UNDECLARED_DEPENDENCY` (il a besoin de l'historique des changements intégrés,
  que la requête d'oracle ne porte pas).

Oracle par défaut : `TextualSemanticsOracle` (`textual-semantics-v1`) — les
heuristiques textuelles existantes (surface publique, marqueurs ABI, assertions
de contrat retirées). **Aucune prétention forte** : il ne voit ni les dépendants
inter-crates, ni les imports ambigus, ni les régressions fonctionnelles ; c'est
pourquoi il ne peut jamais justifier une acceptation à lui seul.

Faux oracles/vérificateurs en test : `tests/collab5_semantic_statuses.rs`
(`ScriptedOracle`, `ScriptedVerifier`), conformément au mandat. Le branchement
réel L1 (analyse inter-crates de `ravum-unit-graph`/daemon) est branché ensuite
par le Lead : il devra renvoyer `Findings` (au moins `INTERFACE_BREAK` bloquant
pour une interface utilisée ailleurs) ou `Unavailable`, jamais un `Clear`
implicite.

### `BoundedProof` (PROVED borné uniquement)

```rust
pub struct BoundedProof {
    property: String, scope: Vec<String>, assumptions: Vec<String>,
    tool: String, tool_version: String,
    target_snapshot: SnapshotId, evidence_id: String,
}
```

`is_explicit()` exige : propriété non vide, périmètre non vide, **au moins une
hypothèse**, outil et version non vides, evidence id de 64 hex ; `applies_to()`
exige le snapshot candidat exact. Sinon le proof est journalisé comme ignoré et
le niveau reste `SEMANTICALLY_CHECKED`/`TESTED`. Aucun `PROVED` implicite.

---

## 6. Les 10 cas du mandat — statuts observés

Corpus : `fixtures/collab5/cases/<id>/` (`scenario.json`, base, patches), exécuté
par `tests/collab5_semantic_statuses.rs`. « Réel » = décidé par le code du
coordinateur/oracle textuel réel ; « scripté » = oracle/vérificateur injecté
(faux), le chemin rustc réel étant couvert par les suites COLLAB-2/3 existantes
(`collab2_two_functions.rs`, `collab3_semantic.rs`, qui exécutent réellement
`rustc`).

| # | cas | voie | décision | niveau | raison journalisée (extrait) | statut |
|---|---|---|---|---|---|---|
| 1 | même fonction modifiée deux fois | vérificateur scripté (1er) | `AUTO_SAFE` puis `CONFLICT` | `SEMANTICALLY_CHECKED` puis `REJECTED` | « symbol "alpha" is already modified by agent-a/alpha-a » | PASS |
| 2 | signature publique utilisée ailleurs | oracle textuel réel | `REBASE_REVALIDATE` | `TEXTUALLY_APPLICABLE` | « interface change used elsewhere: revalidation … required » | PASS |
| 3 | appelant devenu incompatible | vérificateur scripté | `REJECT` | `REJECTED` | « merged snapshot does not compile … E0061 … semantic verification failed » | PASS |
| 4 | contrat affaibli | oracle textuel réel | `MANUAL_REQUIRED` | `MANUAL_REQUIRED` | « oracle finding [WEAKENED_CONTRACT]: contract assertion removed … » | PASS |
| 5 | dépendance non déclarée | vérificateur scripté (1er réel-coordinateur pour le 2e) | `AUTO_SAFE` puis `MANUAL_REQUIRED` | `SEMANTICALLY_CHECKED` puis `MANUAL_REQUIRED` | « undeclared semantic dependency: … symbol "helper" … agent-a » | PASS |
| 6 | import ambigu | oracle scripté | `MANUAL_REQUIRED` | `MANUAL_REQUIRED` | « oracle finding [AMBIGUOUS_IMPORT] … » | PASS |
| 7 | changement d'ABI | oracle textuel réel | `MANUAL_REQUIRED` | `MANUAL_REQUIRED` | « oracle finding [ABI_CHANGE]: ABI surface changed … » | PASS |
| 8 | modification de lockfile | règles réelles (chemin) | `MANUAL_REQUIRED` | `MANUAL_REQUIRED` | « target path "Cargo.lock" is generated or a lockfile » | PASS |
| 9 | régression fonctionnelle compilable | vérificateur scripté | `REJECT` | `REJECTED` | « semantic verification failed: the test harness … failed » | PASS |
| 10 | snapshot périmé | règles réelles (CAS) | `REBASE_REVALIDATE` | `TEXTUALLY_APPLICABLE` | « base snapshot … is stale and expected hashes do not cover … » | PASS |

Tous les cas sont journalisés : une ligne de journal v2 par décision, avec
`evaluation.level`, `evaluation.decision`, la raison complète, l'`oracle` et/ou
le `verification` record et les `evidence_ids` (déterministes). Le test vérifie
la relecture du journal (`read_journal_records`) après chaque soumission.

---

## 7. Commandes exécutées et résultats

Toutes exécutées depuis `D:\App\Ravum` avec `CARGO_BUILD_JOBS=2`, builds
sérialisés sur machine chargée.

### 7.1 Tests du crate

```
cargo test -p ravum-workspace-coordinator -- --test-threads=1
```

Résultat mesuré : **119 tests passés, 0 échoué, 0 ignoré** (code de sortie 0).

| cible | nombre | commentaire |
|---|---|---|
| unitaires `src/lib.rs` | 76 | 49 existants conservés + 27 nouveaux (`evaluation` 8, `oracle` 6, `coordinator` v2 13) |
| `tests/collab5_semantic_statuses.rs` | 5 | nouveau corpus + invariant central + journal v1 |
| `tests/collab1_non_regression.rs` | 2 | COLLAB-1 non-régression (inchangé) |
| `tests/collab2_two_functions.rs` | 3 | rustc réel (inchangé) |
| `tests/collab3_semantic.rs` | 10 | 10 cas COLLAB-3 rustc réel (inchangé) |
| `tests/git_publication.rs` | 16 | publication transactionnelle (inchangé) |
| `tests/git_writer.rs` | 2 | (inchangé) |
| `tests/journal_durability.rs` | 3 | journal v2 accepté par le lecteur v1 (inchangé) |
| `tests/merge_strategies.rs` | 2 | mesure COLLAB-2 (inchangé) |

Les 87 tests existants sont présents et verts : aucun fichier de test existant
n'est modifié dans le diff final.

### 7.2 Build et format

```
cargo build -p ravum-workspace-coordinator   # OK
cargo build -p ravum-cli                     # OK, aucun changement de ravum-cli
rustfmt --edition 2021 --check <fichiers modifiés>   # OK sur les fichiers du lot
```

Aucun warning nouveau : `cargo test`/`cargo build` du crate n'émettent pas de
warning (les fichiers existants non touchés n'ont pas été reformatés).

Consommateur existant vérifié sans modification : `cargo test -p ravum-cli
--test collab_flow -- --test-threads=1` → **3 passés, 0 échoué** (deux agents
intégrés, conflit refusé, vue périmée rejetée) : l'API additive ne change pas
le comportement COLLAB-1 de `ravum-cli`.

### 7.3 Smoke test CLI réel (v2 sans/avec vérification, v1 défaut)

Patch réel sur `fixtures/collab5/cases/01_same_function_twice`, deux
propositions (agent-a et agent-b modifient `alpha`), `ravum-collab integrate` :

```
v2 sans --verify : exit=0
  MANUAL_REQUIRED agent-a/alpha-a   level=TEXTUALLY_APPLICABLE  protocol=V2_SEMANTIC_STATUSES
  MANUAL_REQUIRED agent-b/alpha-b   level=TEXTUALLY_APPLICABLE  protocol=V2_SEMANTIC_STATUSES

v2 avec --verify (rustc réel) : exit=0
  AUTO_SAFE       agent-a/alpha-a   level=SEMANTICALLY_CHECKED  protocol=V2_SEMANTIC_STATUSES
  CONFLICT        agent-b/alpha-b   level=REJECTED              protocol=V2_SEMANTIC_STATUSES

v1 défaut : exit=0
  AUTO_SAFE       agent-a/alpha-a   level=TEXTUALLY_APPLICABLE  protocol=V1_COLLAB_COMPAT
  CONFLICT        agent-b/alpha-b   level=REJECTED              protocol=V1_COLLAB_COMPAT
  accepted record schema=2 (evaluation.protocol=V1_COLLAB_COMPAT, is_legacy_textual_auto_safe=true)
```

C'est l'invariant central démontré sur le vrai binaire : sans vérification
déclarée, le patch textuellement applicable n'est **pas** `AUTO_SAFE`.

---

## 8. Limites honnêtes

1. **Pas de preuve générale.** `PROVED` est toujours borné (propriété + périmètre
   + hypothèses + outil + snapshot) ; il n'existe aucun chemin implicite vers
   `PROVED`, et aucun prouveur externe n'est exécuté dans ce lot.
2. **Oracle L1 non branché.** L'interface `SemanticOracle` est stable et testée
   avec un faux oracle ; l'analyse inter-crates réelle (dépendants d'une
   interface) arrive dans un lot ultérieur côté Lead. Le défaut textuel ne
   détecte ni dépendants inter-crates, ni imports ambigus, ni régressions
   fonctionnelles : ces cas sont soit refusés par prudence (`MANUAL_REQUIRED`),
   soit couverts par un vérificateur déclaré.
3. **Voies scriptées du corpus.** Les cas 1/3/5/9 utilisent un vérificateur
   scripté et le cas 6 un oracle scripté (mandat : faux oracle en test). Ce qui
   est prouvé pour ces cas est la **logique de décision et de journalisation**,
   pas la compilation rustc elle-même ; celle-ci est prouvée séparément par les
   e2e COLLAB-2/3 qui exécutent réellement `rustc` (voir §7.1).
4. **Compatibilité v1 = compatibilité, pas preuve.** Le mode v1 continue
   d'émettre `AUTO_SAFE` pour un patch textuellement applicable ; c'est
   explicitement marqué `V1_COLLAB_COMPAT` + `TEXTUALLY_APPLICABLE` et
   `is_legacy_textual_auto_safe()`. Un consommateur qui ignore ce marquage
   surestimerait la vérification ; les nouveaux consommateurs doivent exiger
   `protocol=V2_SEMANTIC_STATUSES` et un niveau intègre.
5. **Journal v2 = sur-ensemble.** Les lecteurs non-serde doivent ignorer les
   champs inconnus (ou utiliser `read_journal_records`) ; un lecteur strict qui
   rejette l'inconnu casserait sur les lignes v2. Documenté.
6. **Pas de CRDT, pas de fusion concurrente.** Deux modifications de la même
   fonction restent un conflit.
7. **ABI** : non prouvable localement ; refus explicite, pas de verdict
   d'incompatibilité ABI.
8. **Code du snapshot exécuté** quand un vérificateur exécute les tests : ce
   n'est pas un bac à sable (même utilisateur, même machine) ; c'est pourquoi
   cette capacité reste opt-in dans le binaire.
9. **Pas de fusion/commit Git** faite par ce lot ; aucune commande git mutante.

---

## 9. Fichiers du lot

* `crates/ravum-workspace-coordinator/src/evaluation.rs` (nouveau) — schéma v2,
  niveaux, records, journal v2 + lecteur tolérant.
* `crates/ravum-workspace-coordinator/src/oracle.rs` (nouveau) —
  `SemanticOracle`, findings, `BoundedProof`, oracle textuel par défaut.
* `crates/ravum-workspace-coordinator/src/coordinator.rs` — protocole v1/v2,
  gate strict, records d'évaluation, journal v2, 13 tests v2.
* `crates/ravum-workspace-coordinator/src/records.rs` — `AcceptedChange.evaluation`.
* `crates/ravum-workspace-coordinator/src/lib.rs` — exports additifs.
* `crates/ravum-workspace-coordinator/src/bin/ravum-collab.rs` — `--protocol`,
  `evaluations`/`protocol` dans le rapport JSON.
* `crates/ravum-workspace-coordinator/tests/collab5_semantic_statuses.rs` (nouveau).
* `fixtures/collab5/**` (nouveau corpus, 10 cas + README).
* `docs/mvp/MVP-0c-collaboration.md` (ce document).
* `evidence/mvp0c-collab-2026-11-09.json` (preuve machine).

Aucun fichier hors périmètre (pas de `ravum-cli`, `ravum-daemon`,
`ravum-unit-graph`, `Cargo.toml` racine, `.github`).

---

## 10. Prochaines étapes (pour le Lead)

1. Brancher l'implémentation réelle de `SemanticOracle` sur l'analyse
   inter-crates L1 : `INTERFACE_BREAK` bloquant avec la liste des unités
   dépendantes ; `Unavailable` si l'analyse n'est pas fiable (fail-closed).
2. Faire passer les consommateurs (CLI/daemon) en `V2_SEMANTIC_STATUSES` et
   exiger `level.is_integration_grade()` avant activation.
3. Formaliser la revalidation (`REBASE_REVALIDATE` → nouvelle proposition sur le
   snapshot courant, avec le finding conservé) et l'invalidation ciblée côté
   daemon.
4. Ajouter un vrai `BoundedProof` sur un cas DSP (Kani bloqué sous Windows :
   consigner), avec provenance et hypothèses explicites.
5. Mesurer le coût de l'oracle L1 par proposition et publier les chiffres (ne
   pas revendiquer un gain sans mesure).


