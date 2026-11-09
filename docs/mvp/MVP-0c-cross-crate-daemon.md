# MVP-0c — L1.2 : externs contenu-adressés dans `Daemon::analyze`

> **Lot L1.2 — worker B.** Statut : **branché, testé, mesuré**. L'analyse chaînée
> cross-crate est désormais utilisable via l'API publique du daemon ; aucun
> `PROVED`, aucun gain de performance revendiqué sans mesure.
> Preuves brutes : [`evidence/mvp0c-cross-crate-daemon-2026-11-09.json`](../../evidence/mvp0c-cross-crate-daemon-2026-11-09.json).

## 1. Problème corrigé

L1 (commit `cecfa77`) avait livré le crate `ravum-unit-graph`, le manifeste
snapshot schéma 2 (`externs/` contenu-adressés), `CompiledExtern` /
`run_with_externs`, `ExternUsageReport` — mais `Daemon::analyze` n'acceptait
aucun extern : l'analyse chaînée restait hors du produit et le cache ignorait
l'identité des interfaces de dépendances.

L1.2 branche le chemin réel :

1. nouvelle API publique **`Daemon::analyze_with_externs(&AnalysisRequest, &[ExternInput])`**
   (fail-closed) ; `Daemon::analyze` délègue avec une liste vide et reste
   **identique au comportement legacy** (mêmes `snapshot_id`, mêmes clés de cache) ;
2. l'identité du snapshot **inclut les externs** (`name`, `crate_name`,
   `interface_id`, taille) : deux requêtes aux sources identiques mais aux externs
   différents ont des `snapshot_id` différents ;
3. le cache mémoire/disque est indexé par l'identité complète, avec en défense en
   profondeur un **digest d'externs** ajouté à `CacheLookupKey` et au verdict :
   aucun HIT ne peut survivre à un changement d'extern ;
4. `rustc` reçoit réellement `--extern name=<.rmeta>` (`run_with_externs`), et le
   rapport expose les externs utilisés (`externs[].validates()`) ainsi que
   `artifact_sha256` de l'interface produite ou réutilisée ;
5. l'interface produite est stockée **contenu-adressée** dans
   `<work>/artifacts/<sha>.rmeta`, lisible par `Daemon::artifact(interface_id)`
   (SHA-256 revérifié avant retour) : c'est ce qui alimente une analyse dépendante.

## 2. API et format

```rust
// 1) analyser la dépendance (aucun extern)
let core = daemon.analyze(&request_core)?;            // success = true
let interface = core.artifact_sha256.unwrap();        // SHA-256 du .rmeta
let bytes = daemon.artifact(&interface)?;             // octets vérifiés

// 2) analyser le dépendant avec l'interface réelle
let extern_core = ExternInput::new("core", "core", bytes)?;
let api = daemon.analyze_with_externs(&request_api, &[extern_core])?;
assert_eq!(api.externs[0].interface_id, interface);
```

Format wire JSON-lines (côté client, `serde`) :

```json
{ "name": "core", "crate_name": "core",
  "interface_id": "<sha256>", "len": 6224, "rmeta_hex": "…" }
```

Le désérialiseur est **fail-closed** : `interface_id` doit être le SHA-256 exact
des octets décodés, `len` doit correspondre, le nom doit être un identifiant Rust,
la taille est bornée (`MAX_EXTERN_BYTES = 8 Mio`, `MAX_EXTERNS_PER_UNIT = 64`).
Un doublon de nom, un extern absent/altéré ou un `interface_id` incohérent est
refusé **avant** toute compilation (`ErrorKind::InvalidRequest`).

## 3. Identité, snapshot et cache

| Élément | Avant L1.2 | Après L1.2 |
|---|---|---|
| `snapshot_id` | sources + contexte + toolchain | + `externs` (name/crate/interface/len) ; zéro extern = hash legacy inchangé |
| Snapshot manifest | schéma 1 | schéma 2, artefacts `externs/<interface_id>.rmeta` + copie de service `externs/<snapshot_id>/lib<name>.rmeta` (rustc exige `lib*.rmeta`) |
| Clé cache | `(snapshot, contexte, toolchain)` | + digest d'externs ; zéro extern = clé v1 byte-for-byte |
| Verdict | succès/diagnostics | + `externs_digest` + `artifact_sha256` (un HIT continue de servir l'interface) |
| Driver | `run` | `run_with_externs` si externs (sinon `run` inchangé) |

**Closure transitive** : pour décoder une ré-exportation (`api` ré-exporte
`core::Point`), `rustc` a besoin des métadonnées du `core` référencé par `api`.
Le daemon enregistre pour chaque artefact ses externs directs
(`artifacts/<sha>.externs.json`) et matérialise la **fermeture d'interfaces**
(directe + transitive, dédupliquée par `interface_id`) dans
`<work>/externs/<snapshot_id>/`. C'est indispensable pour compiler `consumer`
dans `core → api → consumer`. Un nom d'extern ambigu dans la fermeture est refusé.

## 4. Tests réellement exécutés

`cargo test -p ravum-daemon --offline -- --test-threads=1`
→ **113 tests, 0 échec** (66 lib, 5 bin, 7 auto_gc, 15 daemon_integration,
7 externs_integration, 10 gc_integration, 3 tcp_multi_client).
Les **106 tests existants** sont conservés et verts ; 7 nouveaux tests
d'intégration réels dans `tests/externs_integration.rs` :

| Test | Ce qui est prouvé |
|---|---|
| `real_chain_dependency_rmeta_then_dependent_then_hit` | `.rmeta` réel produit par une analyse daemon → `ExternInput` → dépendant compilé, miss réel puis HIT identique ; `artifact_sha256` servi aussi en HIT |
| `changed_extern_without_source_change_is_a_real_miss_with_correct_diagnostic` | mêmes sources `api` + extern changé (`core` compatible puis cassant) ⇒ **miss** (jamais HIT), `snapshot_id` différent, diagnostic `E0061` réel quand la signature casse |
| `extern_validation_is_fail_closed` | doublon/interface falsifiée/octets vides/nom invalide refusés ; artefact altéré refusé (`SnapshotCorrupt`) |
| `legacy_schema_one_snapshot_still_verifies_without_externs` | manifeste schéma 1 lisible et vérifié sans externs |
| `transitive_extern_closure_decodes_re_exports` | `consumer` compile `api::Point` grâce à la fermeture `core`→`api` |
| `extern_digest_is_order_independent_and_interface_sensitive` | digest stable par ensemble, sensible à l'interface |
| `extern_inputs_roundtrip_through_json_protocol_shape` | format wire hex accepté, interface falsifiée refusée à la désérialisation |

Le test `real_chain_*` vérifie aussi `report.externs[].validates()` et
`report.snapshot.externs == 1`.

## 5. Mesure du coût (bench, chiffres réels)

Commande :

```powershell
$env:CARGO_BUILD_JOBS=2
cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite cross-crate
```

`fixtures/multicrate` : `core` → `api` (extern core) → `consumer` (extern api),
3 rounds, analyses et `.rmeta` réels. Machine partagée chargée : calibration
`rustc -vV` mesurée à **190/184/175 ms** au début de la campagne (à indiquer dans
toute lecture des temps).

| Campagne | rustc invoqués | Temps (3 rounds) | Snapshots (round froid) |
|---|---|---|---|
| **isolée** (core seul, sans extern) | 1 (round 1), puis 2 HIT | round 1 : **237 ms total / 228 ms analyse** ; rounds 2–3 : **0 ms** (HIT) | **1 231 B** |
| **chaînée** (core → api → consumer) | 3 (round 1), puis 6 HIT | round 1 : **681 ms** (core 266, api 196, consumer 202) ; rounds 2–3 : **8–9 ms** (HIT) | core **1 231 B**, api **6 224 B**, consumer **3 399 B** |

Changement d'extern (mêmes sources `api`, nouvelle interface `core`) :
**miss réel**, `analysis_ms = 228`, nouveau `snapshot_id` et nouveau snapshot
`6 357 B` — aucun HIT périmé, comportement attendu et prouvé.

Lecture honnête : la chaîne cross-crate **coûte plus cher** que l'analyse isolée
d'une unité (3 compilations + artefacts + fermeture) ; elle n'est pas une
optimisation de vitesse et ne remplace pas L2. Son intérêt est la **correction**
(validation d'interface réelle, invalidation des dépendants) et la réutilisation
des verdicts par identité complète. Aucun gain n'est revendiqué.

## 6. Limites et non-claims

- **Pas de `PROVED`** : les statuts/reports sont des observations bornées.
- Un seul toolchain/target (rustc 1.95.0, x86_64-pc-windows-msvc), machine
  partagée chargée (calibration incluse dans l'évidence) ; 3 rounds seulement.
- Fixture minuscule ; seules les cibles `lib` sont chaînées ; binaires, features,
  build scripts et dépendances externes restent sur le fallback Cargo de L1.
- La closure transitive est construite à partir des artefacts **produits par ce
  daemon** (`artifacts/*.externs.json`). Un extern fourni par un tiers n'a pas de
  sidecar : ses dépendances transitives doivent être fournies explicitement, sinon
  `rustc` échoue avec un diagnostic réel (jamais une interface inventée).
- `AnalysisRequest` n'a **pas** été modifié (aucun champ ajouté) afin de ne pas
  casser les 106 tests existants ni le protocole des clients : l'extension est
  additive (`analyze_with_externs` + `ExternInput` sérialisable pour un futur
  branchement CLI/JSON-lines). Divergence assumée et documentée ; le Lead peut
  demander un champ `externs` dans `AnalysisRequest` s'il préfère ce contrat.
- Cache local JSON, pas de cache distribué ; un ancien verdict sans digest
  d'externs ne peut jamais matcher une requête avec externs (miss fail-closed).
- Détection `include!`/`env!` toujours textuelle (L1) et sur-conservative.
- Aucun test existant supprimé ni affaibli ; un seul test unitaire snapshot de la
  mission précédente a vu sa **mise en place** adaptée au nouveau contrat
  d'identité (`identity_with_externs`), avec assertions renforcées (les deux
  assertions supplémentaires vérifient que l'extern change le `snapshot_id`).

## 7. Fichiers touchés

| Fichier | Changement |
|---|---|
| `crates/ravum-daemon/src/daemon.rs` | `analyze_with_externs`, identité/cache avec externs, `run_with_externs`, store d'artefacts, closure transitive, rapport |
| `crates/ravum-daemon/src/cache.rs` | `externs_digest` dans `CachedVerdict`/`CacheLookupKey`, `artifact_sha256` ; clé v1 préservée sans externs |
| `crates/ravum-daemon/src/snapshot.rs` | `ExternInput` sérialisable (hex), `identity_with_externs`, `externs_digest`, validation publique |
| `crates/ravum-daemon/src/report.rs` | `AnalysisReport.externs` + `artifact_sha256`, `SnapshotReport.externs` |
| `crates/ravum-daemon/src/lib.rs` | ré-exports (`ExternInput`, `validate_externs`, `externs_digest`, `SnapshotExtern`) |
| `crates/ravum-daemon/tests/externs_integration.rs` | 7 tests d'intégration réels (nouveau fichier) |
| `crates/ravum-daemon/src/bin/ravum-daemon-bench/{main,cross_crate}.rs` | suite `cross-crate` + preuve JSON |
| `docs/mvp/MVP-0c-cross-crate-daemon.md` | ce rapport |

## 8. Externs dans le protocole JSON-lines (L1.3)

> **Statut : branché, testé de bout en bout (stdio + TCP), compatible legacy.**
> `AnalysisRequest` n'a **pas** changé : le protocole accepte un champ optionnel
> `externs` au niveau de la requête wire, et le serveur appelle
> `Daemon::analyze_with_externs`. Aucun littéral Rust ni client existant n'est
> cassé. Preuve brute :
> [`evidence/mvp0c-daemon-wire-externs-2026-11-09.json`](../../evidence/mvp0c-daemon-wire-externs-2026-11-09.json).

### 8.1 Schéma JSON exact

Requête legacy (inchangée, `externs` absent = liste vide) :

```json
{ "id":"1", "cmd":"analyze", "revision":1, "label":"api", "root":"src/lib.rs",
  "files":[{"path":"src/lib.rs","content":"pub fn scaled(x: f64) -> f64 { core::scale(x) }\n"}],
  "context":{"edition":"2021","crate_name":"api","crate_type":"lib","flags":[]} }
```

Requête avec externs (champ additionnel optionnel) :

```json
{ "id":"2", "cmd":"analyze", "revision":2, "label":"api", "root":"src/lib.rs",
  "files":[{"path":"src/lib.rs","content":"pub fn scaled(x: f64) -> f64 { core::scale(x) }\n"}],
  "context":{"edition":"2021","crate_name":"api","crate_type":"lib","flags":[]},
  "externs":[ { "name":"core", "crate_name":"core",
                "interface_id":"e32c9adbd78ce1ca…(64 hex)",
                "bytes_hex":"7f454c46…(le .rmeta encodé en hexadécimal)" } ] }
```

Réponse (enveloppe existante) : `{"id","ok":true,"wall_ms","report"}` où `report`
contient `externs` (nom, crate, `interface_id`, `rmeta_sha256`, `validates`),
`artifact_sha256` (interface produite/réutilisée, réutilisable comme
`interface_id` d'une analyse dépendante) et, comme avant, `snapshot_id`,
`cache`, `diagnostics`, `timings`.

Erreur (même enveloppe, aucun panic, aucun état partiel) :

```json
{"id":"3","ok":false,"error":{"kind":"invalid_request","message":"extern \"core\": interface_id does not match the SHA-256 of bytes_hex"}}
```

Types Rust (dans `snapshot.rs`, ré-exportés) :
`WireAnalyzeRequest { #[serde(flatten)] request: AnalysisRequest, #[serde(default)] externs: Vec<WireExtern> }`
et `WireExtern { name, crate_name, interface_id, bytes_hex }` (`deny_unknown_fields`).
`WireAnalyzeRequest::into_parts()` valide et convertit en `Vec<ExternInput>`.
Le serveur stdio et le serveur TCP utilisent le même chemin de parsing, donc le
résultat est identique sur les deux transports.

### 8.2 Validation fail-closed (avant toute compilation)

- `externs` absent → `Vec::new()` : comportement legacy strictement identique
  (`snapshot_id` inchangé pour un même contenu).
- `externs` non-tableau → `invalid_request`.
- élément non conforme au schéma (champ inconnu, `bytes_hex` non-hex, longueur
  impaire) → `invalid_request`.
- `interface_id` ≠ SHA-256 des octets décodés → `invalid_request`.
- nom/crate non identifiant Rust, octets vides, > `MAX_EXTERN_BYTES` (8 Mio),
  > `MAX_EXTERNS_PER_UNIT` (64), noms dupliqués → `invalid_request`.
- une requête invalide ne modifie pas la révision courante observée par les
  requêtes suivantes et le daemon continue de servir (testé).

### 8.3 Résultats mesurés (évidence, rustc 1.95.0, machine partagée)

| Étape protocole | Résultat |
|---|---|
| stdio, analyse legacy | `ok`, `fresh`, `success`, miss, `snapshot_id=ad9071c6f8b4…`, 110 ms |
| stdio, même requête legacy répétée | **HIT**, `snapshot_id` identique, 1 ms |
| stdio, `api` **sans** externs | miss, `success=false`, `snapshot_id=4f6ff34f341d…` |
| stdio, `api` **avec** extern `core` v1 | miss, `success=true`, `snapshot_id=c25092c7f388…` (**différent**), 109 ms, `externs=1` |
| stdio, même requête avec extern répétée | **HIT**, `snapshot_id` identique, 5 ms |
| stdio, hex invalide / SHA incohérent / nom dupliqué | 3 × `ok=false`, `error.kind=invalid_request`, aucun crash |
| stdio, requête valide après les erreurs | `ok`, `success=true` (daemon opérationnel) |
| TCP, `api` + extern v1 | HIT (cache du work_dir partagé), `snapshot_id` identique |
| TCP, même requête répétée | HIT, 1 ms |
| TCP, extern **compatible** v2 (mêmes sources) | **miss réel**, `success=true`, nouveau `snapshot_id=eec5d12d22f0…`, 126 ms |
| TCP, extern **cassant** v3 (mêmes sources) | miss réel, `success=false`, diagnostic rustc réel `E0061`, 131 ms |

Conclusion : le protocole expose bien l'analyse inter-crates (un client
stdio/TCP peut fournir un `.rmeta` réel et compiler un dépendant), l'identité
complète est respectée (un changement d'extern = miss, jamais HIT périmé), la
compatibilité ascendante est prouvée, et les erreurs sont explicites.

### 8.4 Limites et non-claims

- `bytes_hex` transporte le `.rmeta` complet : la taille est bornée à 8 Mio par
  extern et 64 externs par unité ; au-delà, refus explicite.
- Un extern fourni par un tiers n'a pas de sidecar `artifacts/*.externs.json` :
  ses propres dépendances transitives doivent être fournies explicitement, sinon
  rustc produit un diagnostic réel (jamais une interface inventée par le daemon).
- Le protocole reste **loopback/stdin** ; pas de TLS, pas d'authentification, pas
  de multi-machine.
- Aucun `PROVED` ; pas de test multi-heure ; un seul toolchain/target mesuré.
- Deux tests d'intégration protocolaires ajoutés (`wire_externs.rs`), stdio et TCP
  réels ; la suite daemon passe de 113 à **115 tests verts**.
