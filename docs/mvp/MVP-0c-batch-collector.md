# MVP-0c — Collecteur de lots borné avec pré-vol de conflits (`ravum-batch`)

Statut : **IMPLÉMENTÉ / TESTÉ** (L2 phase 2) — pas une release, pas un `PROVED`.
S'appuie sur les mesures et seuils de [`MVP-0c-build-performance.md`](MVP-0c-build-performance.md)
(sections 8–9) et sur la preuve
[`../evidence/mvp0c-batch-2026-11-09.json`](../../evidence/mvp0c-batch-2026-11-09.json).

## 1. Objectif

Les mesures L2 montrent que N éditions compatibles analysées ensemble coûtent
1 analyse au lieu de N (−74 à −83 %), mais qu'un lot refusé coûte plus cher
qu'une analyse par proposition (184 ms de refus + ~1 596 ms de reprise mesurée).
Le collecteur matérialise la recommandation : regrouper **uniquement** des
propositions textuellement disjointes sur une même base, avec une fenêtre
bornée, un pré-vol de conflits sans `rustc`, un repli conservateur et une
télémétrie permettant de mesurer la fréquence réelle `p`.

Hors périmètre : aucune modification du daemon, du CLI, du coordinateur ni de
`ravum-unit-graph` ; aucune dépendance à `rustc` dans ce crate.

## 2. API (`crates/ravum-batch/src/lib.rs`)

```rust
pub trait BatchProposal {            // pré-vol, sans rustc
    fn proposal_id(&self) -> &str;   // id externe unique (ex. agent/task)
    fn paths(&self) -> &[String];    // chemins ciblés
    fn symbols(&self) -> &[String];  // symboles / ancres textuelles
    fn base_id(&self) -> &str;       // base CAS, non vide
}
pub trait Analyzer {                 // injecté (daemon/coordinateur côté CLI)
    fn analyze(&mut self, ids: &[&str]) -> Verdict;   // lot ou item isolé
    fn diagnose(&mut self, id: &str) -> Verdict { ... } // feedback immédiat
}

pub struct BatchCollector<P: BatchProposal, A: Analyzer> { ... }
impl BatchCollector {
    pub fn new(config: BatchConfig, analyzer: A) -> Self;
    pub fn submit(&mut self, proposal: P, now_ms: u64) -> Result<ItemId, SubmitError>;
    pub fn feedback(&mut self, item_id: ItemId) -> Result<Verdict, FeedbackError>;
    pub fn poll(&mut self, now_ms: u64) -> Vec<BatchReport>;
    pub fn next_batch(&mut self, now_ms: u64) -> Option<BatchReport>;
    pub fn flush(&mut self, now_ms: u64) -> Vec<BatchReport>;
    pub fn telemetry(&self) -> &BatchTelemetry;
    pub fn pending_items(&self) -> usize;
    pub fn pending_batches(&self) -> usize;
}
```

- **Configuration bornée** : `window_ms` défaut **50**, maximum **100** ;
  `max_batch` défaut **6**, maximum **8** ; `max_pending` (défaut 256) borne la
  file. `BatchConfig::new` rejette toute valeur hors bornes (`ConfigError`).
- **Temps injecté** : le collecteur ne lit jamais l'horloge ; `submit`, `poll`,
  `next_batch`, `flush` prennent `now_ms: u64` (horloge monotone fournie par
  l'appelant). Les tests n'utilisent aucun `sleep` et sont déterministes.
- **`submit` non bloquant** : valide les métadonnées, effectue le pré-vol,
  place la proposition et retourne son `ItemId` monotone ; aucune analyse n'est
  exécutée.
- **`poll`/`next_batch`** exécutent les lots échus (fenêtre expirée ou lot
  plein), dans l'ordre des `batch_id`.
- **`feedback(item_id)`** : diagnostic immédiat hors lot (compteur
  `feedback_calls`), refusé (`AlreadyAnalyzed`) une fois le lot exécuté.
- **Ids monotones** : `ItemId` et `batch_id` sont strictement croissants ; les
  `proposal_id` externes dupliqués sont rejetés explicitement.

## 3. Pré-vol de conflits (sans rustc)

Un lot n'accepte une proposition que si, pour **chacune** de ses propositions
déjà présentes :

1. même `base_id` (même base CAS) ;
2. aucun chemin commun (`paths`) ;
3. aucun symbole commun (`symbols`).

La recherche est un **first-fit** déterministe dans l'ordre de création des lots
ouverts. Une proposition incompatible avec tous les lots ouverts ouvre un lot
séparé (isolation) : elle n'est jamais fusionnée. La télémétrie compte
`preflight_isolations`, `path_conflicts`, `symbol_conflicts` et `base_mismatches`.

Limite explicite : le pré-vol est **textuel** (chemins/symboles/base). Un
conflit sémantique non déclaré (ex. signature utilisée ailleurs) n'est pas
détectable ici ; il est découvert par l'analyseur et déclenche le repli.

## 4. Repli conservateur et non-perte

Si l'analyse groupée échoue (`Verdict.ok == false`) :

- **aucune acceptation partielle** : le lot est marqué `FellBack` et n'accepte
  rien ; les `accepted_ids` ne proviennent que des analyses individuelles ;
- chaque proposition du lot est ré-analysée individuellement (**coût N+1** :
  1 analyse de lot refusée + N analyses individuelles) ;
- chaque proposition finit **exactement une fois** acceptée ou refusée ; le
  rapport porte `analyses_used` (1 ou N+1) ;
- `flush` ferme tous les lots ouverts (raison `Flush`) et exécute le reste ;
- l'invariant `telemetry.no_loss()` vérifie `proposals_received ==
  items_reported` (aucune perte silencieuse).

## 5. Télémétrie (`BatchTelemetry`)

`proposals_received`, `proposals_rejected` (invalides/dupliqués/file pleine),
`batches_formed`, `batches_accepted_all`, `batches_fell_back`,
`individual_analyses`, `analyses_used`, `analyses_saved_net`
(`Σ(taille − analyses_used)`, négatif pour un repli), `feedback_calls`,
`preflight_isolations`, `path_conflicts`, `symbol_conflicts`,
`base_mismatches`, `wait_samples_ms` (p50/p95 nearest-rank), `items_reported`.
Ce sont des événements observés, jamais des estimations.

## 6. Tests déterministes (aucun `sleep`, aucun rustc)

`cargo test -p ravum-batch -- --test-threads=1` → **19 tests verts**
(4 unitaires de configuration/percentile/invariant + **15 tests d'intégration**
avec analyseur factice). Couverture :

| Garantie testée | Test |
|---|---|
| N compatibles → 1 analyse | `six_compatible_proposals_share_one_analysis` |
| borne `max_batch`, ids monotones, ordre stable | `max_batch_is_bounded_and_ids_are_monotonic` |
| fenêtre expirée → lots distincts + latence mesurée | `window_expiry_separates_batches_and_records_wait` |
| lot plein → pas d'attente de fenêtre | `a_full_batch_does_not_wait_for_the_window` |
| conflit de symbole isolé, jamais fusionné | `a_textual_symbol_conflict_is_isolated_never_merged` |
| conflit de chemin isolé | `a_shared_path_is_isolated` |
| base CAS différente → jamais fusionnée | `a_different_cas_base_is_never_merged` |
| lot refusé → repli N+1, zéro acceptation partielle | `a_refused_batch_falls_back_to_n_plus_one_without_partial_acceptance` |
| repli ne rejette que la proposition fautive | `fallback_rejects_only_the_failing_proposal` |
| aucun flux mixte ne perd de proposition | `no_proposal_is_lost_in_a_mixed_stream` |
| feedback immédiat disponible avant le lot | `immediate_feedback_is_available_before_the_batch` |
| rejets invalides/dupliqués/file pleine explicites | `invalid_duplicate_and_overfull_submissions_are_rejected_explicitly` |
| `next_batch` ordonné par `batch_id` | `next_batch_returns_batches_in_batch_id_order` |
| fenêtre 0 = fermeture immédiate | `zero_window_expires_immediately` |
| scénario mixte p≈50 % (4 lots sur 8 en repli) | `mixed_scenario_about_half_of_the_batches_fall_back` |

## 7. Mesure sur arrivées simulées (sans rustc, sans cargo dans les boucles)

Binaire `ravum-batch-measure` : 3 scénarios, timestamps injectés, analyseur
déterministe (aucun `sleep`, aucune dépendance machine). Configuration mesurée :
fenêtre 50 ms, `max_batch` 6.

| Scénario | propositions | lots | replis | analyses utilisées | économie nette | attente p50 / p95 | isolations pré-vol |
|---|---:|---:|---:|---:|---:|---:|---:|
| `compatible-5x6` (5 vagues pleines) | 30 | 5 | 0 | **5** | **+25** | 5 / 5 ms | 0 |
| `mixed-p-50` (4 vagues sur 8 avec rejet imprévu) | 40 | 8 | **4 (50 %)** | **28** | **+12** | 50 / 50 ms | 0 |
| `textual-conflicts` (symbole+chemin partagés) | 18 | 12 | 0 | **12** | **+6** | 50 / 50 ms | **6** (path 6, symbol 6) |

Aucune perte : `proposals_received == items_reported` dans les trois scénarios ;
les appels d'analyseur observés égalent `analyses_used` ; les conflits textuels
ne sont jamais fusionnés (12 lots de taille 2 et 1 au lieu de 6 lots de 3).

### 7.1 Comparaison au modèle L2.2 (MODÈLE calibré, pas une mesure bout en bout)

Coûts de phase 1 : cycle d'analyse **191 ms**, reprise d'un lot refusé
**1 596 ms** (mesurée à N=6, conservatrice pour N=5), fenêtre **50 ms**.

Scénario mixte mesuré (p = 50 %, N = 5) : coût séquentiel modèle
`40 × 191 = 7 640 ms` ; coût collecté modèle `7 548 ms` → **gain 92 ms (1,2 %)**.
Le résultat est **marginal** parce que la constante de reprise (1 596 ms, N=6)
est conservatrice pour des lots de 5 et que le p≈50 % est proche du seuil.

Balayage du même modèle à p = 50 %, w = 50 ms :

| N | séquentiel | groupé attendu | gain | rentable à p=50 % | seuil p* |
|---:|---:|---:|---:|---|---:|
| 3 | 573 ms | 943,5 ms | −370,5 ms | **non** | 23,6 % |
| 4 | 764 ms | 943,5 ms | −179,5 ms | **non** | 37,2 % |
| 5 | 955 ms | 943,5 ms | +11,5 ms | **oui (marginal)** | 50,8 % |
| 6 | 1 146 ms | 943,5 ms | +202,5 ms | **oui** | 64,4 % |
| 7 | 1 337 ms | 943,5 ms | +393,5 ms | oui | 78,0 % |
| 8 | 1 528 ms | 943,5 ms | +584,5 ms | oui | 91,6 % |

**Verdict honnête** : le collecteur fait exactement ce qu'il promet — il réduit
le nombre d'analyses (25 économies sur 30 éditions compatibles, 12 sur 40 en
mixte à 50 % de replis, 6 sur 18 en présence de conflits textuels pré-vol) et ne
perd aucune proposition. En revanche, **à p≈50 % le gain temporel dépend
fortement de N** : il est **négatif pour N ≤ 4**, marginal à N=5 et net à N≥6.
C'est cohérent avec les seuils L2.2 ; le collecteur n'améliore ni la correction
ni le coût d'un lot refusé — il rend seulement le regroupement **sûr et
mesurable**. À p≈50 %, un déploiement doit donc viser N=6 (défaut) et non des
lots de 3–4.

## 8. Recommandation d'intégration (CLI, par le Lead)

1. Envelopper les `ChangeProposal` du coordinateur dans un adaptateur
   `BatchProposal` (id = `proposal_id`, paths = `target_paths`, symbols =
   `target_symbols`, base = `base_snapshot_id`).
2. Analyseur : `analyze(ids)` = intégration groupée + analyse daemon des unités
   affectées ; `diagnose(id)` = diagnostics déjà disponibles (feedback immédiat).
3. Boucle : `submit` à chaque proposition reçue ; `poll(now)` sur timer/événement ;
   `flush` à l'arrêt ; exporter `telemetry()` pour mesurer `p` réel.
4. Ne grouper que si la télémétrie/le pré-vol estime `p` sous le seuil
   `p*(N, window)` (tableau L2.2) ; sinon analyse individuelle directe.
5. Repli : appliquer le rapport `FellBack` sans acceptation partielle et
   journaliser le refus du lot (le collecteur ne décide pas la politique
   d'acceptation ; il fournit les verdicts).

## 9. Limites et non-claims

- Mesure sur **arrivées simulées** et analyseur déterministe : aucun `rustc`,
  aucun cargo dans les boucles, aucune mesure machine réelle ; les ms sont un
  **modèle calibré** issu de la phase 1, pas une mesure bout en bout.
- La reprise est modélisée par la constante mesurée à N=6 (conservatrice pour
  N<6) ; un modèle par analyse donnerait un gain supérieur pour N=5, non
  revendiqué.
- Pré-vol **textuel** uniquement (chemins/symboles/base exacts) : un conflit
  sémantique non déclaré n'est pas anticipé, il passe par le repli.
- Un repli coûte N+1 analyses et n'améliore jamais le coût ; il garantit
  seulement l'absence d'acceptation partielle et la non-perte.
- Le collecteur est **synchrone/poll-based** (pas de runtime async) ; la
  latence d'attente télémetrée est celle du lot, pas la latence utilisateur.
- Aucun test existant modifié ; le crate n'est pas encore branché au CLI
  (intégration réservée au Lead).
- Aucun `PROVED` : un lot `AcceptedAll` signifie « analyseur a accepté », pas
  « programme correct ».

## 10. Reproduire

```powershell
cargo test -p ravum-batch -- --test-threads=1
cargo run -q -p ravum-batch --bin ravum-batch-measure
# preuve : evidence/mvp0c-batch-2026-11-09.json
cargo build --workspace --offline
```

Fichiers : `crates/ravum-batch/{Cargo.toml,src/lib.rs,src/bin/ravum-batch-measure.rs,tests/collector.rs}`,
une ligne member dans `Cargo.toml` racine (+ `Cargo.lock`), ce document et la
preuve JSON.
