# Corpus `fixtures/collab5` — cas négatifs de collaboration (MVP-0c L3)

Ce corpus alimente `crates/ravum-workspace-coordinator/tests/collab5_semantic_statuses.rs`.
Il contient les **dix cas négatifs du mandat MVP-0c L3**, chacun dans
`cases/<id>/` avec :

* `scenario.json` — métadonnées lues par le test : id, description, voie
  d'exécution (`lane`), fichiers de base, propositions (agent, symbole, patch,
  préconditions), décisions et niveaux attendus, findings d'oracle scriptés,
  spécification de vérificateur scripté, sous-chaînes de raison attendues ;
* `src/lib.rs` (et éventuellement d'autres fichiers de base, ex. `Cargo.lock`) ;
* `*.patch` — diff unifié exact, appliqué par le coordinateur.

Le test vérifie que chaque cas produit **la décision et le niveau d'évaluation
attendus**, que la raison est journalisée et que le journal écrit est un record
v2 relisible (`read_journal_records`).

## Voies (`lane`) et honnêteté des preuves

| lane | ce qui est réellement exécuté |
|---|---|
| `textual_refusal` | règles réelles du coordinateur (politique de chemin, CAS/base périmée) — aucun acteur scripté. |
| `textual_default_oracle` | oracle textuel par défaut réel (`TextualSemanticsOracle`) : surface publique, ABI, assertion de contrat retirée. |
| `scripted_oracle` | faux oracle injecté (le mandat demande un faux oracle en test ; le branchement L1 réel arrive ensuite côté Lead). |
| `scripted_verifier` | faux vérificateur injecté (compilation/tests simulés). Le chemin **réel** `rustc` est déjà couvert de bout en bout par `tests/collab2_two_functions.rs` et `tests/collab3_semantic.rs`. |

Aucun cas sans vérificateur déclaré n'attend `AUTO_SAFE` : le test
`the_corpus_contains_the_ten_mandate_cases` le refuse explicitement.

## Cas

| id | cas du mandat |
|---|---|
| `01_same_function_twice` | même fonction modifiée deux fois |
| `02_public_signature_used_elsewhere` | signature publique modifiée utilisée ailleurs |
| `03_incompatible_caller` | appelant devenu incompatible |
| `04_weakened_contract` | contrat affaibli (assertion retirée) |
| `05_undeclared_dependency` | dépendance non déclarée |
| `06_ambiguous_import` | import ambigu |
| `07_abi_change` | changement d'ABI |
| `08_lockfile` | modification de lockfile |
| `09_functional_regression` | régression fonctionnelle dans du code qui compile |
| `10_stale_snapshot` | agent sur snapshot périmé |
