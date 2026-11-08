# M2 — Preuves, ledger et invalidation (résultats réels)

> Ce document rapporte des exécutions réelles du 2026-11-08, commit de code
> **`e0a7114`**. Ce qui n'a pas été exécuté est marqué comme tel. Aucun `PROVED`
> général n'est revendiqué : seuls des domaines finis explicitement énumérés le sont.

## 1. Ce qui est implémenté

| Composant | Rôle | Fichier |
|---|---|---|
| `Obligation` / `Claim` | propriété + domaine fini explicite + hypothèses | `crates/ravum-proof/src/obligation.rs` |
| `verification_key` | clé dérivée du contexte **complet** | idem |
| `ExhaustiveVerifier` | énumère le domaine déclaré sur l'artefact réel | `verifier.rs` |
| `KaniVerifier` | détection d'un prouveur externe, refus honnête | idem |
| `EvidenceLedger` | journal append-only, verdicts `Fresh`/`Stale` | `ledger.rs` |
| `ProofBroker` | cache + exécution + enregistrement | `broker.rs` |
| Gate d'activation | refuse la publication si une obligation n'est pas satisfaite | `crates/ravum-coordinator/src/pipeline.rs` |

Le vocabulaire de claims est volontairement étroit : `Finite`, `Deterministic`,
`NonNegative`, `Equals`, `MonotonicNonDecreasing`. Un claim non vérifiable doit
produire `Unknown`, jamais un succès silencieux.

## 2. Vérification réelle (CLI)

```powershell
ravum prove fixtures/proof/positive.rs --claim finite --inputs 0,1,2.5,1000000 --ledger evidence/m2-ledger.jsonl --json
ravum prove fixtures/proof/negative.rs --claim non-negative --inputs 0,1,2,3 --ledger evidence/m2-ledger.jsonl --json
```

Sorties brutes : `evidence/m2-proof.json`, journal : `evidence/m2-ledger.jsonl`.

| Cas | Statut | Passé | Contre-exemple | Exit |
|---|---|---|---|---|
| `positive.rs`, `finite` sur 4 entrées | `proved` | oui | — | 0 |
| `negative.rs`, `non_negative` sur 4 entrées | `tested` | non | **`f(2) = -1 < 0`** | 1 |

Le contre-exemple est **précis** (entrée et valeur), pas un échec vague : c'est ce
qu'exige P0-E. Un échec est enregistré avec le statut `tested` et `passed=false`,
jamais requalifié en succès.

### 2.1 Portée exacte du `proved`

L'enregistrement porte, dans `assumptions` :

- `declared domain: 4 explicit input(s); nothing outside it is covered` ;
- `trust base: rustc-built artifact, the native loader, and this harness (ravum-exhaustive 0.1.0)` ;
- le temps de vérification.

Autrement dit : la propriété est prouvée **sur ces quatre entrées**, par énumération
exhaustive, en faisant confiance au compilateur, au chargeur natif et au harnais.
Ce n'est pas une preuve pour tout `f64`, et le code ne permet pas de le prétendre :
un domaine vide renvoie `Unsupported` (« enumerating nothing proves nothing »).

## 3. Invalidation du cache : mutations réellement testées

`crates/ravum-proof/tests/proof_cache.rs` et `src/obligation.rs` (unitaires).

| Mutation | Effet observé sur la clé | Comportement | Test |
|---|---|---|---|
| octets de l'artefact | clé différente | nouvelle exécution | `mutating_the_artifact_invalidates_the_cache` |
| domaine d'entrées | clé différente | nouvelle exécution | `mutating_the_domain_or_the_assumptions_invalidates_the_cache` |
| hypothèses déclarées | clé différente | nouvelle exécution | idem |
| version d'outil | clé différente | nouvelle exécution + ancien verdict `Stale` | `mutating_the_toolchain_invalidates_and_marks_the_old_verdict_stale` |
| identifiant d'obligation | clé différente | nouvelle exécution | `key_changes_with_every_mutated_field` |
| contexte (snapshot/build/artefact) | clé identique mais contexte différent | `Stale`, re-vérification obligatoire | `another_context_is_stale_not_fresh` |
| statut `Stale` enregistré | — | jamais `Fresh` | `a_stale_status_is_never_fresh` |
| ledger corrompu | — | ouverture refusée (fail-closed) | `corrupt_ledger_is_refused` |

Aucun de ces tests ne se contente d'observer la clé : chacun vérifie le **nombre
d'exécutions du vérificateur** (`verifier_runs`) ou le verdict retourné.

## 4. Cache d'artefacts (nécessaire pour que M2 ait un sens)

Mesure qui a motivé le cache (`crates/ravum-rust-adapter/tests/artifact_cache.rs`) :
**deux `rustc` sur la même source ne produisent pas les mêmes octets** sous MSVC.
Vérifié aussi à la main : deux `sha256` différents pour `positive.rs`.
`/Brepro` et `/timestamp:0` n'y changent rien.

Conséquence : un cache de preuve indexé sur `ArtifactId` serait toujours froid.
Le cache d'artefacts donne donc un identifiant stable par `(snapshot, contexte)`.

| Situation | `build_ms` | `cached_build` | `artifact_id` |
|---|---|---|---|
| premier build (froid) | **2 286** | faux | `dcdc4682…` |
| second build (chaud) | **3** | vrai | `dcdc4682…` (identique) |

Source : `evidence/m2-build-cache.json`. Un artefact modifié sur disque invalide le
cache (test `a_tampered_cached_artifact_is_rebuilt_not_trusted`).

Effet secondaire positif : on ne réécrit **jamais** un DLL qu'un processus a chargé
(impossible sous Windows), ce qui corrige un défaut réel : rebâtir la même version
échouait au link.

## 5. Gate d'activation

`crates/ravum-cli/tests/m2_proof_gate.rs` (4 tests) :

1. une obligation violée **bloque** l'activation et laisse la version active intacte ;
2. un domaine plus étroit laisse passer le **même** artefact (le gate est exactement
   aussi large que le domaine déclaré — conséquence assumée, pas un bug) ;
3. déclarer des obligations sans fournir de broker est une **erreur**, jamais un saut ;
4. une seconde demande identique réutilise le verdict (`verifier_runs` inchangé) et
   se heurte ensuite à `AlreadyActive`.

## 6. Performances : le seuil n'est pas atteint

Mesures `ravum bench` (20 itérations sauf mention), cache d'artefacts **désactivé**
pour que chaque build soit un vrai `rustc`. La colonne « calibration » est le temps
médian d'un simple `rustc -vV` mesuré dans le même run : elle caractérise la charge
machine, pas le coût de l'analyse.

| Run | Calibration | `total` p50 | `total` p95 | `total` p99 |
|---|---|---|---|---|
| A (cycle M1, sans calibration) | inconnue | 380 ms | 481 ms | 544 ms |
| B | 83 ms | 576 ms | 778 ms | 1 560 ms |
| C | 80 ms | 533 ms | 2 078 ms | 2 440 ms |
| D (10 iter.) | 91 ms | 716 ms | 796 ms | 796 ms |
| E (10 iter.) | 99 ms | 766 ms | 908 ms | 908 ms |
| F (10 iter.) | 136 ms | 902 ms | 1 218 ms | 1 218 ms |
| **G (dernier, versionné)** | **96 ms** | **525 ms** | **746 ms** | **817 ms** |
| G, détail | — | min **502 ms** | — | — |

Source versionnée : `evidence/m1-bench.json` (run G).

### Verdict honnête

| Cible exploratoire (`docs/roadmap/MVP.md`) | Mesure | Verdict |
|---|---|---|
| diagnostic local p95 < 50 ms | min 132 ms, p50 145 ms | **NON ATTEINTE** (facteur ~3) |
| petit patch local p95 < 500 ms | min 502 ms, p95 746 ms | **NON ATTEINTE** |

Le run A (p95 481 ms) avait été présenté comme « atteint de justesse ». **Cette
conclusion est retirée** : ce run n'avait pas de calibration, et aucune des six
mesures instrumentées ne reproduit ce chiffre, même en min. La machine est chargée
par des applications utilisateur (CPU ~66 %, calibration 80–136 ms pour un simple
`rustc -vV`), ce qui interdit toute affirmation p95 sur cet environnement.

**Cause structurelle** : chaque édition lance un `rustc` à froid. Le plancher est le
démarrage du compilateur (calibration 80–136 ms), pas le coût de l'unité analysée.
Aucun cache de requêtes, aucun daemon, aucun frontend incrémental n'existe encore.

C'est un résultat négatif utile, et il alimente directement M5 : la cible P0-A ne
peut pas être atteinte par l'option A (Rust + outillage) telle quelle.

## 7. Dette restante

| Élément | État |
|---|---|
| Prouveur externe (Kani) | **absent** de la machine : `Unsupported`, aucun harness généré |
| Preuve non bornée | **impossible** avec ce harnais (et non revendiquée) |
| Timeout dur | vérifié **après** l'appel : un patch qui bloque indéfiniment bloque le vérificateur |
| Coût du ledger | pas de compaction ; croissance linéaire |
| Benchmark sous charge contrôlée | **non réalisé** (machine partagée) |

## 8. Reproduire

```powershell
cargo build --workspace
cargo test --workspace -- --test-threads=1
cargo run -q --bin ravum -- prove fixtures/proof/positive.rs --claim finite --inputs 0,1,2.5,1000000 --json
cargo run -q --bin ravum -- prove fixtures/proof/negative.rs --claim non-negative --inputs 0,1,2,3 --json
cargo run -q --bin ravum -- bench fixtures/calculator/v1.rs fixtures/calculator/v2.rs --iterations 20
```
