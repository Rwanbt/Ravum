# MVP-0b — Daemon persistant d'analyse incrémentale

> **Statut : L2 implémenté, mesuré et testé.** Ce document rapporte uniquement des
> exécutions réelles. Chaque chiffre provient d'un fichier de `evidence/` listé en
> §2, produit sur la machine de référence sous la charge indiquée. Ce qui n'a pas
> été exécuté est marqué « non exécuté » ou « non démontré » — jamais
> « devrait fonctionner ».
>
> Résultat principal, énoncé sans arrondir : le cache de résultats du daemon fait
> passer l'analyse d'un buffer **identique** de ~139–153 ms (rustc, p50) à
> **< 1 ms** ; pour une **modification réelle** d'une petite unité (~230 lignes),
> `-C incremental` persistant **n'apporte pas de gain** et le daemon paie 5–8 ms
> d'overhead par rapport à son propre `rustc`. Le gain de l'incrémental apparaît
> à partir de ~1 000 lignes : −12 % à −24 %, puis −30 % à −36 % au-delà de
> ~3 000 lignes (mesuré, §5).

## 1. Objet

Le MVP-0a lançait un `rustc` **à froid** à chaque édition. Le plancher mesuré
était le démarrage du compilateur (`rustc -vV` : 80–136 ms), pas le travail
d'analyse ; le cache d'artefacts existant ne s'appliquait qu'aux builds de
contenu identique et ne couvrait pas les diagnostics. Le Lot L2 devait fournir un
**moteur persistant entre deux éditions** et mesurer honnêtement ce qu'il apporte
sur une modification réelle.

Réponse synthétique :

| Question | Réponse mesurée |
|---|---|
| Buffer identique : analyse quasi immédiate ? | **oui** — cache contenu p50 0 ms, p95 0 ms (résolution 1 ms) contre 139–153 ms pour rustc |
| Modification réelle d'une petite unité : plus rapide ? | **non** — daemon p50 165–208 ms contre rustc froid 144–201 ms ; l'incrémental persistant seul ne gagne rien à cette taille |
| Modification réelle d'une unité moyenne/grande : plus rapide ? | **oui, en mode `-C incremental`** — −30 % à −36 % au-delà de ~3 200 lignes (analyse seule), mesuré §5 |
| Diagnostics identiques à rustc froid ? | **oui** — 0 divergence sur toutes les cellules de la campagne et sur les cas négatifs |
| Cible « diagnostic local p95 < 50 ms » | atteinte **pour un contenu inchangé** (0–1 ms), **non atteinte pour un contenu modifié** (p50 165–208 ms) |

## 2. Contexte d'exécution et preuves

| Champ | Valeur |
|---|---|
| Date | 2026-11-08, 14:08–14:11 (heure locale) |
| Branche | `feat/mvp0b-hardening`, HEAD `245db0b844de6c93de5044b43f03b0d654e4c537` |
| OS | Windows 11 (10.0.x), `x86_64-pc-windows-msvc` |
| CPU | 13th Gen Intel Core i7-13620H, 10 cœurs physiques / 16 logiques |
| RAM totale / libre observée | 15,71 Go / **3,4–3,7 Go libres** pendant les campagnes finales |
| rustc | `rustc 1.95.0 (59807616e 2026-04-14)`, LLVM 22.1.2, host `x86_64-pc-windows-msvc` |
| Parallélisme `rustc` autorisé | **1** (`DaemonConfig::max_concurrent_jobs = 1`) ; campagne strictement séquentielle |
| Processus concurrents observés | 329–342 (machine partagée avec d'autres agents MVP-0b) |
| Fixture campagne | `crates/ravum-daemon/fixtures/unit` — 4 modules, 230 lignes, 6 233 octets |
| Fixtures échelle | unités synthétiques déterministes 1 089 / 3 209 / 7 629 lignes (§5) |

Fichiers de preuve bruts (aucun n'est édité à la main) :

| Fichier | Contenu | `generated_unix` |
|---|---|---|
| `evidence/mvp0b-incremental-bench.json` | campagne 7 catégories × 3 modes × 10 échantillons, échantillons individuels, stats, charge par catégorie, état du daemon | 1794143394 |
| `evidence/mvp0b-incremental-scale.json` | expérience d'échelle cold vs incrémental persistant, 3 tailles × 2 cas × 5 répétitions | 1794143337 |
| `evidence/mvp0b-incremental-invariants.json` | 14 contrôles négatifs/limites exécutés (périmé, annulation, corruption, redémarrage, budgets, JSON, fichiers utilisateur) | 1794143489 |

Chaque fichier embarque sa propre charge : `load_start`, `load_end` et, pour la
campagne, `load_end` par catégorie (CPU, mémoire libre, nombre de processus,
calibration `rustc -vV`). Les campagnes n'ont pas été faites dans des conditions
identiques : la charge varie et est reportée, jamais moyennée entre fichiers.

## 3. Architecture livrée

Crate `crates/ravum-daemon` (lib + 2 binaires), ajouté à `members` du
`Cargo.toml` racine. Dépendances : `serde`, `serde_json`, `sha2` uniquement
(déjà verrouillées par le workspace). **Aucune API privée de rustc**, aucun
`RUSTC_BOOTSTRAP`, aucun wrapper hérité : uniquement la CLI publique.

### 3.1 Modules

| Module | Rôle |
|---|---|
| `config.rs` | `BuildContext` (edition, crate-name, crate-type, flags), `ToolchainInfo` (requêté une fois via `rustc -vV`), `Priority`, budgets durs, validation fail-closed des requêtes et des flags réservés |
| `hash.rs` | SHA-256 avec préfixage de longueur (pas de collision par déplacement de frontières) |
| `snapshot.rs` | `AnalysisRequest`, identité de contenu, `SnapshotStore` immuable, `sync_unit_dir` (unité de compilation stable) |
| `cache.rs` | cache de verdicts L1 mémoire (LRU borné) + L2 disque (JSON auto-digéré), `CacheLookupKey` |
| `diagnostics.rs` | parseur `rustc --error-format=json` (formats `$message_type` et `reason=compiler-message`), normalisation des chemins, signatures comparables |
| `revision.rs` | révisions monotones, `CancelToken`, registre des jobs en vol |
| `gate.rs` | admission bornée et priorisée (interactive avant background), `peak` observable |
| `rustc_driver.rs` | lancement `rustc` (`--emit=metadata`, `--error-format=json`, `--diagnostic-width`, `-C incremental=<dir>`), pipes drainés, polling annulable, timeout dur |
| `daemon.rs` | orchestration : validation → identité → cache → gate → snapshot/unité → `rustc` → verdict → rapport ; péremption et annulation |
| `report.rs` | `AnalysisReport` JSON (statut, diagnostics, timings par étape, cache) et `DaemonStats` |
| `sysinfo.rs` | observabilité de charge (Win32 `GlobalMemoryStatusEx`, `GetSystemTimes`, Toolhelp32, `GetProcessMemoryInfo`) — best-effort, `None` si non observé |
| `gc.rs` | GC de rétention **explicite** (P0-H) : `GcPolicy` sérialisable, inventaire cache/snapshots/état incrémental, protections du contenu vivant, suppression sûre sous verrou, `GcReport` JSON (§12) |
| `fsutil.rs` | écriture atomique, `safe_join` anti-traversée |

### 3.2 Identités (toutes SHA-256)

- `toolchain_id = SHA256(version moteur ∥ release ∥ host ∥ commit-hash)`
- `context_id = SHA256(version moteur ∥ edition ∥ crate-name ∥ crate-type ∥ flags ∥ toolchain_id)`
- `snapshot_id = SHA256(version moteur ∥ root logique ∥ context_id ∥ toolchain_id ∥ fichiers triés (chemin, sha256, octets))`
  — le **label d'affichage n'entre pas** dans l'identité (un label ne change pas le contenu).
- `cache_key = SHA256(schema cache ∥ snapshot_id ∥ context_id ∥ toolchain_id)`

Deux conséquences vérifiées : changer un flag (`-D warnings`) ou l'identité de
toolchain change la clé → **miss** ; re-soumettre les mêmes octets → **hit**.

### 3.3 Cycle d'une requête

1. **Acceptation** — validation du contrat (chemins logiques relatifs sans `..`,
   tailles, flags réservés refusés), hachage de contenu, écriture du snapshot
   immuable `<work>/snapshots/<sha256>/checkout/...` (jamais écrasé : une
   divergence est une erreur `SnapshotCorrupt`, pas une réécriture).
2. **Cache lookup** — L1 mémoire puis L2 disque ; une entrée dont le digest
   interne ne correspond plus est traitée comme **miss** (fail-closed).
3. **Admission** — gate borné (`max_concurrent_jobs`, défaut 1) + priorité ;
   verrou par contexte de build (l'état incrémental rustc n'est pas partageable
   entre invocations concurrentes du même crate).
4. **Analyse** — si miss : miroir des seuls fichiers modifiés dans l'unité de
   compilation **stable** `<work>/units/<context>/...` (mêmes chemins d'un édit à
   l'autre, comme un éditeur qui réenregistre le même fichier), puis `rustc`
   avec `-C incremental=<work>/incremental/<context>`. Un changement de contexte
   change les deux répertoires.
5. **Réponse** — rapport JSON avec `status` :

```
{"schema_version":1,"status":"fresh","revision":7,"current_revision":7,
 "unit":"body#3","root":"src/lib.rs","snapshot_id":"…","context_id":"…",
 "toolchain":{"release":"1.95.0","host":"x86_64-pc-windows-msvc","commit_hash":"…"},
 "incremental_dir":"…/incremental/…","cache":{"hit":false,"kind":"miss","entries":12},
 "success":true,"exit_code":0,"diagnostics":[{"severity":"error","code":"E0061",
   "message":"…","file":"src/pipeline.rs","line":11,"column":42,"end_line":11,"end_column":42,
   "rendered":"…"}],
 "timings":{"accept_ms":3,"queue_ms":0,"lookup_ms":0,"analysis_ms":160,"total_ms":165},
 "snapshot":{"files":4,"bytes":6233,"written_files":1,"removed_files":0,"verified_files":4,"dir":"…"},
 "daemon_version":"0.1.0","message":null}
```

### 3.4 Révisions, péremption, annulation

- `revision` est **monotone** : `note(rev)` ne fait jamais reculer la révision
  courante. Une requête dont la révision est strictement antérieure est rejetée
  **avant tout hachage ou compilation** (`status:"superseded"`, diagnostics vides).
- Une révision plus récente **annule réellement** les jobs en vol plus anciens :
  le driver consulte un `CancelToken` toutes les 2 ms et **tue le processus
  rustc** ; le rapport sort en `status:"cancelled"`. Si la réponse arrive juste
  avant la bascule, elle est classée `superseded` et **n'est jamais retournée
  comme courante**.
- Le verdict d'un contenu effectivement compilé reste mis en cache même si sa
  révision a été dépassée : le cache est adressé par contenu, pas par révision ;
  seule la *réponse au client* est filtrée.

### 3.5 API lib et binaire de démonstration

L'API lib est utilisable directement par les tests, sans binaire :

```rust
let daemon = Daemon::open(DaemonConfig { work_dir, max_concurrent_jobs: 1, ..Default::default() })?;
let report = daemon.analyze(&AnalysisRequest { revision, label, root, files, context, priority })?;
if report.is_current() { /* seuls ces diagnostics sont « courants » */ }
```

Le binaire `ravum-daemon` est un transport JSON-lines minimal sur la même API :
`analyze`, `advance_revision`, `stats`, `clear_cache`, `command_template`,
`shutdown` ; une ligne d'événement `ready` d'abord. Il est testé de bout en bout
(restart possible en relançant le processus sur le même `--work-dir`).

## 4. Protocole de mesure

Trois modes, **même contenu**, **mêmes flags rustc publics**, **même fixture** :

| Mode | Ce qui est mesuré |
|---|---|
| `cold` | temps mural d'un processus `rustc --emit=metadata --error-format=json`, `--out-dir` neuf à chaque appel, **sans** `-C incremental` — le comportement MVP-0a |
| `incremental` | même commande avec `-C incremental=<dir>` **persistant** entre les échantillons, chemins de sources stables |
| `daemon` | appel `Daemon::analyze` de bout en bout (validation + SHA-256 + snapshot + sync de l'unité + lookup + rustc si besoin) ; `timings.analysis_ms` isole le processus rustc |

Règles gelées dans chaque fichier de preuve :

- 10 échantillons enregistrés par catégorie et par mode, après **un warm-up non
  enregistré** par mode (sauf `cold`, sans état) ;
- ordre des modes **tournant** par échantillon (cold/incremental/daemon, puis
  incremental/daemon/cold, …) pour répartir la dérive de charge ;
- percentiles **rang le plus proche**, sans interpolation ; à 10 échantillons
  p95 = 10ᵉ valeur, estimation de queue grossière ;
- **correction** : les jeux de signatures de diagnostics (sévérité, code, fichier
  logique, ligne, colonne, fin, message) doivent être **identiques** dans les
  trois modes ; une divergence rend la campagne invalide (code de sortie 1) ;
- aucune mesure de codegen, de link, de vérification d'artefact ni d'activation :
  c'est l'**étape d'analyse** seule.

Commandes exactes :

```powershell
cargo build -p ravum-daemon --offline
cargo test  -p ravum-daemon --offline -- --test-threads=1
cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite campaign --samples 10
cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite scale    --scale-reps 5
cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite invariants
# binaire de démonstration, protocole JSON-lines sur stdin/stdout :
cargo run -q -p ravum-daemon --bin ravum-daemon -- --work-dir target/ravum-daemon
```

Commande rustc de référence réellement utilisée (citée depuis
`command_template` du daemon) :

```
rustc --edition=2021 --crate-name=acme_unit --crate-type=lib --emit=metadata --error-format=json
      --diagnostic-width=120 --out-dir=<work>/out/<context> -C incremental=<work>/incremental/<context>
      <work>/units/<context>/src/lib.rs
```

## 5. Résultats mesurés

### 5.1 Campagne — 7 catégories, 10 échantillons par cellule

**Charge pendant la campagne** (fichier `mvp0b-incremental-bench.json`) :
début CPU 30,3 %, 3 614 Mo libres, 338 processus, calibration `rustc -vV`
p50 = 68 ms (81/68/65) ; fin CPU 37,4 %, 3 503 Mo libres, 340 processus,
calibration p50 = 84 ms (90/84/79). Par catégorie : CPU 21,5–43,8 %, mémoire
libre 3 443–3 581 Mo, 329–340 processus, calibration p50 65–94 ms. Les trois
modes tournent dans la même fenêtre : les écarts ne sont pas attribuables à deux
campagnes différentes.

Toutes les cellules : 0 divergence de diagnostics (`consistency_failures = 0`),
0 échec inattendu (`unexpected_failures = 0`).

| Catégorie (édition cumulative) | cold min/p50/p95 (ms) | +`-C incremental` persistant min/p50/p95 | daemon min/p50/p95 | daemon `analysis_ms` p50 | cache |
|---|---|---|---|---|---|
| (a) `identical` — même buffer | 138 / **153** / 166 | 125 / **139** / 153 | 0 / **0** / 0 | 0 | 10× mémoire |
| (b) `body` — corps de fonction | 139 / **144** / 165 | 146 / **154** / 189 | 143 / **165** / 204 | 160 | 10× miss |
| (c) `signature` — signature publique + appelants | 145 / **157** / 165 | 152 / **177** / 217 | 161 / **183** / 261 | 176 | 10× miss |
| (d) `type` — struct/enum + appelants | 149 / **180** / 213 | 172 / **202** / 241 | 0 / **194** / 249 | 188 | 1 hit (contenu reversé), 9 miss |
| (e) `constant` — constantes | 154 / **201** / 224 | 164 / **201** / 246 | 199 / **208** / 269 | 200 | 10× miss |
| (f) `import` — imports/ré-exports | 156 / **166** / 212 | 157 / **173** / 235 | 0 / **183** / 205 | 176 | 1 hit (contenu reversé), 9 miss |
| (g) `invalid_repair` — buffer incomplet puis réparé | 86 / **123** / 184 | 107 / **121** / 156 | 0 / **0** / 126 | 0 | 10 hits (réparation) / 10 miss (invalide) |

Détail utile de la catégorie (g), par phase :

| Phase | cold min/p50/p95 | incremental min/p50/p95 | daemon min/p50/p95 |
|---|---|---|---|
| buffer incomplet (erreur de syntaxe) | 86 / **92** / 123 | 107 / **113** / 148 | 91 / **104** / 166 |
| buffer réparé (= contenu déjà analysé) | 139 / **155** / 203 | 118 / **125** / 173 | 0 / **0** / 0 |

Lectures honnêtes :

- **Le cache du daemon supprime le compilateur** : 0 ms (toutes valeurs < 1 ms,
  arrondies à 0 par une mesure en millisecondes entières) contre 139–155 ms pour
  rustc. C'est le seul chemin qui atteint la cible « diagnostic p95 < 50 ms ».
- **Pour une modification réelle d'une unité de 230 lignes, `-C incremental`
  persistant ne gagne rien** : 154–202 ms (p50) contre 144–201 ms à froid, soit
  égal ou légèrement plus lent. Le coût de chargement/sauvegarde de la session
  incrémentale (répertoire 8,57 Mo après 59 compilations) annule le peu de
  requêtes réutilisables à cette taille.
- **Sur un miss, le daemon paie 5–8 ms** par rapport à son propre `rustc`
  (`daemon.p50 − analysis_ms.p50` = 5 / 7 / 6 / 8 / 7 ms selon la catégorie) :
  hachage, écriture du snapshot, synchronisation de l'unité stable, insertion du
  verdict, granularité de polling 2 ms. C'est le prix de l'identité de contenu et
  de la réutilisabilité.
- Les deux « hits » des catégories `type` et `import` ne sont pas des tricheries :
  l'une des éditions cumulatives **annule exactement** une édition précédente
  (ajout puis retrait d'une variante ; ajout puis retrait d'un import), donc le
  contenu redevient identique à un état déjà analysé. Le cache étant adressé par
  contenu, le hit est correct — et le prouve (`min = 0 ms`).
- `invalid_repair` montre les deux régimes : l'analyse d'un buffer cassé est
  rapide (échec précoce, 92–104 ms p50), la réparation d'un buffer **déjà
  analysé** est instantanée (0 ms) alors que rustc froid repaie 155 ms.

### 5.2 Expérience d'échelle — quand `-C incremental` devient rentable

**Charge** : début CPU 42,1 %, 3 624 Mo libres, 341 processus, calibration
84/92/79 ms ; fin CPU 26,7 %, 3 530 Mo libres, 342 processus, calibration
64/63/82 ms. 5 répétitions par cellule, ordre cold/incrémental alterné,
génération synthétique déterministe (coût de génération non mesuré).

| Taille | Lignes | Contenu identique — cold p50 | identique — incr. p50 | Δ | Corps modifié — cold p50 | corps — incr. p50 | Δ | Répertoire incr. |
|---|---|---|---|---|---|---|---|---|
| small | 1 089 | 280 ms | **213 ms** | **−24 %** | 282 ms | **249 ms** | **−12 %** | 3,84 Mo |
| medium | 3 209 | 511 ms | **339 ms** | **−34 %** | 443 ms | **312 ms** | **−30 %** | 10,11 Mo |
| large | 7 629 | 709 ms | **453 ms** | **−36 %** | 635 ms | **444 ms** | **−30 %** | 24,58 Mo |

Toutes les répétitions ont produit des diagnostics identiques entre les deux
modes (`diagnostics_consistent = true`). Les échantillons individuels sont dans
le fichier de preuve (la dispersion sous charge est visible : p. ex. large
identique cold 657–948 ms).

Conclusion : sur cette machine/toolchain, le point de bascule observé se situe
**entre la fixture de campagne (~230 lignes, aucun gain) et ~1 100 lignes**
(−12 % à −24 %) ; au-delà de ~3 200 lignes le gain stabilise autour de −30 % à
−36 %. Ce seuil est une observation locale, pas une garantie portable.

### 5.3 Granularité d'invalidation et tests négatifs

Contrôles exécutés par `--suite invariants` (14/14) — fichier
`mvp0b-incremental-invariants.json`, charge CPU 73–80 %, calibration 101–130 ms :

| Contrôle | Résultat | Preuve clé |
|---|---|---|
| `cross_module_signature_invalidation` | **PASS** | signature de `scale` changée sans mettre à jour l'appelant : rustc froid **et** daemon rapportent `E0061` dans `src/pipeline.rs` (ligne 11, colonne 42), signatures identiques ; après mise à jour de l'appelant, les deux compilent |
| `local_body_edit_then_cache_hit` | **PASS** | édition de corps : compilation miss puis second envoi identique = hit mémoire, `analysis_ms = 0`, diagnostics identiques à froid |
| `context_flags_invalidate_cache` | **PASS** | `-D warnings` sur contenu identique ⇒ miss (recompilation réelle) ; retour au contexte initial ⇒ hit |
| `toolchain_identity_invalidates_cache` | **PASS** | identité de toolchain forcée via la couture de test ⇒ miss ; aucun second toolchain installé (non testé comme tel) |
| `stale_revision_never_current` | **PASS** | révision 13 demandée alors que courante = 18 ⇒ `superseded`, **0 diagnostic retourné** |
| `superseded_during_compilation` | **PASS** | nouvelle révision pendant la compilation ⇒ **`cancelled`**, message « compiler process killed: revision 2 superseded by 3 », 35 ms de compilation en vol avant le kill ; l'analyse suivante du même contenu est `fresh` et identique à froid |
| `cache_corruption_degrades_to_miss` | **PASS** | entrée disque altérée ⇒ `miss`, recompilation réelle, diagnostics corrects, `cache_corrupt = 1` |
| `restart_reuses_persistent_state` | **PASS** | après redémarrage : deux contenus déjà analysés = hit **disque** ; nouveau contenu = miss recompilé correctement (répertoire incrémental présent avant/après) |
| `process_budget_enforced` | **PASS** | 3 requêtes concurrentes, `max_concurrent_jobs = 1` ⇒ `peak_active_jobs = 1`, 3 `rustc_runs`, 3 réponses fraîches |
| `memory_cache_budget` | **PASS** | `max_cache_entries = 1` ⇒ 1 entrée mémoire, 2 verdicts disque adressés par contenu |
| `analysis_timeout_reported` | **PASS** | budget 1 ms dépassé ⇒ `timed_out`, jamais retourné comme courant |
| `json_report_contract` | **PASS** | sérialisation/désérialisation JSON, champs requis et timings présents |
| `user_files_untouched` | **PASS** | fichier utilisateur soumis → octets sur disque inchangés (le daemon n'écrit que sous son `work_dir`) |
| `granularity_observable_proxy` | **PASS** | édition locale : 184 ms froid vs 196 ms daemon (analyse 188 ms), diagnostics identiques ; **caveat** : le temps est un proxy, la CLI publique de rustc n'expose pas de rapport par requête |

Suite `cargo test` : **50 tests, 50 réussis, 0 échec** —
32 tests unitaires bibliothèque, 3 tests des chaînes d'édition du binaire de
mesure, 15 tests d'intégration (dont protocole binaire, corruption de snapshot,
péremption, annulation, timeout, budgets, fichiers utilisateur).

## 6. Comparaison honnête MVP-0a vs MVP-0b

Les nombres MVP-0a proviennent de `docs/mvp/M1-evidence.md` §4.1 corrigé et
`docs/mvp/M2-evidence.md` §6 (fixture `calculator` ~700 octets, run G). Les
nombres MVP-0b viennent de la présente campagne (fixture `unit` 6,7 Ko /
230 lignes). **Ce ne sont pas les mêmes fixtures ni les mêmes fenêtres de
charge** : la comparaison porte sur l'ordre de grandeur et la structure du coût,
pas sur un rapport exact.

| Mesure | MVP-0a | MVP-0b (cette campagne) | Lecture |
|---|---|---|---|
| Analyse `rustc` à froid (diagnostic) | min 132 ms, p50 145 ms | cold p50 **144–201 ms** selon catégorie | même plancher de démarrage, fixture différente |
| cache de contenu identique (analyse) | inexistant (cache d'artefacts de build seulement) | daemon p50 **0 ms**, p95 0 ms | gain majeur sur re-run |
| `-C incremental` persistant | inexistant | p50 139–202 ms à 230 lignes (aucun gain) ; −12 à −36 % au-delà de ~1 100 lignes | dépend de la taille de l'unité |
| patch total (analyse + build + link + verify) | p50 525 ms, p95 746 ms | **non mesuré** en MVP-0b (daemon = analyse seule) | la cible « patch p95 < 500 ms » reste non démontrée |
| cible diagnostic p95 < 50 ms | non atteinte (min 132 ms) | atteinte **seulement** pour un contenu inchangé (0–1 ms) ; sinon 165–261 ms | résultat partiel explicite |
| calibration machine (`rustc -vV`) | 80–136 ms | 63–130 ms selon la fenêtre | machine partagée, charge documentée par fichier |

## 7. Goulots identifiés (chiffrés)

1. **Démarrage de processus** : 63–94 ms p50 de calibration pendant la campagne
   finale (jusqu'à 130 ms sous charge), pour un simple `rustc -vV`. C'est le
   plancher de toute invocation froide ou incrémentale.
2. **Session incrémentale** : à 230 lignes, le chargement/sauvegarde du
   répertoire incrémental (8,57 Mo après 59 compilations) coûte plus qu'il ne
   rapporte : +7 % à +13 % sur p50 par rapport au froid. Le gain n'apparaît
   qu'au-delà de ~1 100 lignes (12–24 %) et atteint 30–36 % au-delà de ~3 200.
3. **Overhead du daemon sur miss** : 5–8 ms p50 (hachage + snapshot + sync +
   insertion cache + polling 2 ms). Il est compensé instantanément sur un hit
   (< 1 ms), mais rend un miss légèrement plus lent que rustc froid.
4. **Aucun `rustc` en processus** : le daemon reste un orchestrateur de CLI
   publique ; il ne peut donc pas passer sous le plancher de démarrage pour un
   contenu modifié. Les 0 ms viennent du cache, pas du moteur.

### 7.5 Empreinte du daemon au repos (mesurée, 2026-11-08)

Le binaire `ravum-daemon` a été lancé sur un `work_dir` vide, sans aucune requête, et
observé pendant 6 secondes (processus mesuré par `Get-Process` : WorkingSet et temps CPU) :

| Métrique au repos | Valeur |
|---|---|
| RAM (WorkingSet) | **4,78 Mo** (stable sur 6 s) |
| CPU consommé | **0,016 s** cumulées sur 6 s (≈ 0 %) |
| Threads | 4 |
| État disque initial | 0 fichier (les sous-répertoires sont créés, vides) |

Ordres de grandeur comparables pour la même machine : la campagne L2 a produit 9,05 Mo
d'état pour 87 requêtes / 59 compilations, et 3,8–24,6 Mo de répertoires incrémentaux selon
la taille d'unité. Ces chiffres restent **locaux et non bornés par un GC** : la croissance
disque est le coût principal, pas la mémoire résidente. Aucun test de fuite longue
(soak) n'a été exécuté.
5. **Croissance disque** : 9,05 Mo pour 87 requêtes / 59 compilations (dont
   8,57 Mo d'état incrémental) ; 24,58 Mo pour l'unité de 7 629 lignes
   après 6 compilations incrémentales. Aucun GC ni compaction n'est implémenté.

## 8. Limites et ce qui n'est PAS démontré

- **Pas de preuve de granularité au niveau requête** : la CLI publique de rustc
  n'expose pas de rapport de réutilisation par requête. La granularité est
  démontrée par la **correction** (un changement de signature est bien répercuté
  sur l'appelant) et par un **proxy temporel** ; pas par une trace interne.
- **Cible « diagnostic local p95 < 50 ms » non atteinte pour une modification**
  (p50 165–208 ms sur la fixture). Seul le contenu inchangé l'atteint.
- **Cible « petit patch p95 < 500 ms » non mesurée** : le daemon ne couvre que
  l'analyse ; codegen, link, vérification et activation restent hors périmètre.
- **Budget mémoire/processus** : admissible seulement au niveau interne
  (`peak_active_jobs = 1`, cache mémoire borné, `rustc_runs` comptés). Aucun
  profilage RSS sous charge longue, aucun test de fuite (« soak test »).
- **Annulation** : le kill a été observé une fois (35 ms en vol), avec un polling
  de 2 ms ; pas de caractérisation statistique de la latence d'annulation.
- **Toolchain** : l'invalidation par changement de toolchain est testée via la
  couture de test (identité forcée) ; aucun second toolchain n'a été installé ni
  benchmarké.
- **Priorité interactive** : testée unitairement (admission avant background) et
  présente dans le gate ; aucune mesure de préemption perçue par un éditeur.
- **Concurrence multi-processus** : deux daemons partageant le même `work_dir`
  n'est pas testé (le snapshot gère la course par vérification, pas mesuré) ; les
  requêtes du même contexte sont sérialisées par verrou, les contextes
  différents peuvent tourner jusqu'à `max_concurrent_jobs` (non benchmarké).
- **Révisions non persistées** : un daemon redémarré repart à la révision 0 ; la
  persistance couvre les verdicts de contenu et le répertoire incrémental.
- **Cache disque non borné (état L2 historique)** : adressé par contenu, aucun LRU
  disque automatique. Dans la campagne L2, les snapshots et `rmeta` s'accumulaient
  (9 Mo pour 87 requêtes) sans GC. Un GC de rétention **explicite** a depuis été
  ajouté et mesuré (§12) : plafonds, protections du contenu vivant, rapport JSON ;
  il reste **manuel** (pas de GC automatique en tâche de fond) et ne compacte pas
  l'état interne de rustc.
- **Sondes machine Windows uniquement** : sur une autre plateforme, la charge
  n'est pas observée (`None` explicite), le code compile mais n'est pas mesuré.
- **Éditeur/overlay réel non intégré** : la sémantique « buffer non sauvegardé »
  est couverte par l'API (contenu fourni), mais aucun client LSP/éditeur n'est
  branché ni mesuré.
- **Fixtures** : 230 lignes pour la campagne, unités synthétiques pour l'échelle.
  Aucune preuve sur un vrai crate multi-crates ou macro-lourd.
- **Comparaisons inter-campagnes** : les fichiers de preuve ont été produits à
  des moments différents de charge (campagne : CPU 21–44 % ; échelle : 26–42 % ;
  invariants : 73–80 %). Chaque chiffre est reporté avec sa propre charge, aucun
  mélange n'est fait sans le dire.

## 9. Reproductibilité et fichiers ajoutés

Ajouts (non commités, le Lead committe) :

- `crates/ravum-daemon/` (crate complet : lib, 2 binaires, fixture
  `fixtures/unit`, 1 fichier de tests d'intégration) ;
- `Cargo.toml` racine : une ligne `"crates/ravum-daemon",` dans `members`
  (`Cargo.lock` mis à jour par cargo) ;
- `evidence/mvp0b-incremental-bench.json`,
  `evidence/mvp0b-incremental-scale.json`,
  `evidence/mvp0b-incremental-invariants.json` ;
- `evidence/mvp0b-daemon-gc-2026-11-08.json` (GC P0-H, §12),
  `scripts/demo-daemon-gc.ps1` (démonstration, propage le code de sortie) ;
- `crates/ravum-daemon/src/gc.rs`, `crates/ravum-daemon/tests/gc_integration.rs`,
  `crates/ravum-daemon/src/bin/ravum-daemon-bench/gc.rs` (mesure GC) ;
- ce document.

Vérifications finales exécutées :

```powershell
cargo build -p ravum-daemon --offline          # OK
cargo test  -p ravum-daemon --offline -- --test-threads=1   # 68/68 tests OK
```

Le total est passé de 50 à **68 tests** (50 historiques inchangés + 9 tests
unitaires GC + 9 tests d'intégration GC réels).

## 10. Intégration dans la CLI (ajoutée après la campagne, 2026-11-08)

`ravum check <fichier> --daemon` route l'analyse par le daemon persistant au lieu du
`rustc` à froid du pipeline MVP-0a. État persistant sous `target/ravum/daemon` ; la
sortie JSON ou humaine inclut `status`, `cache` (`miss` / `memory` / `disk`), et les
latences par étape. Le chemin à froid reste disponible (`ravum check` sans `--daemon`).

Mesuré sur la machine de référence (branche `feat/mvp0b-hardening`, après l'ajout) :

| Commande | Résultat |
|---|---|
| `ravum check fixtures/dsp/iir_v1.rs --daemon` (1ʳᵉ fois) | `status=fresh cache=miss analysis=182 ms total=187 ms` |
| même commande (2ᵉ fois) | `status=fresh cache=disk analysis=0 ms total=1 ms` |
| `ravum check target/ravum-tests/daemoncli-broken.rs --daemon` | 2 diagnostics `error`, **exit 1** |
| `ravum check fixtures/dsp/iir_v1.rs` (chemin à froid, non régressé) | `check 201 ms`, aucun diagnostic |

Ce que cela démontre : le moteur persistant réduit réellement à ~1 ms l'analyse d'un
contenu déjà vu, et l'échec d'analyse est correctement remonté (exit 1). Ce que cela ne
démontre **pas** : la cible « diagnostic p95 < 50 ms sur une modification réelle » reste
non atteinte (voir §6), et le temps total édition→diagnostic→patch→activation n'est pas
encore mesuré de bout en bout par la CLI.

**Mise à jour (même journée)** : la boucle complète a depuis été mesurée par
`ravum cycle-bench` — voir [MVP-0b-cycle.md](MVP-0b-cycle.md). Résultat : contenu
inchangé, cycle complet 85–177 ms → **8–13 ms** ; édition réelle 316–600 ms, dominée par
le build+link à froid, MVP-0b non plus rapide. C'est cohérent avec les chiffres
d'analyse ci-dessus et cela déplace le goulot identifié vers le build/link.

## 11. Prochaines étapes proposées

1. **Brancher le daemon dans la boucle Ravum** (CLI/overlay) pour mesurer le
   temps total d'édition→diagnostic→patch, seule métrique qui compte (P0-A), et
   non plus l'analyse seule.
2. **Stratégie adaptative** : activer `-C incremental` seulement au-delà du seuil
   mesuré (~1 000 lignes, à recalibrer) ; en dessous, garder le cache de verdicts
   sans l'état incrémental pour supprimer les +7–13 % observés.
3. **Réduire l'overhead miss** : réutiliser l'unité stable sans recopier le
   snapshot quand seul un fichier change, écrire la cache L2 en asynchrone,
   polling d'annulation 1 ms — puis remesurer.
4. **GC/compaction** : **fait pour le GC explicite** (§12) — plafonds
   cache/snapshots/état incrémental, `keep_recent`, dry-run, mesure réelle.
   Restent ouverts : GC automatique en tâche de fond, compaction interne de
   l'état rustc, soak test de plusieurs heures.
5. **Robustesse** : deux daemons sur un même `work_dir`, crash en cours
   d'écriture, redémarrage avec état incrémental partiel — tester et documenter.
6. **Étendre la mesure d'échelle** à un vrai crate multi-crates et à du code
   macro/traits lourd, pour vérifier que le seuil de rentabilité tient.

## 12. GC de rétention borné et budget disque (P0-H, 2026-11-08)

> **Statut : implémenté, testé, mesuré.** Le GC est **explicite uniquement** : une
> analyse ne déclenche jamais de collecte. Il protège le contenu vivant (révisions
> récentes, dernier contenu servi, dernier contexte de build) et rapporte
> honnêtement un plafond qu'il ne peut pas atteindre plutôt que de supprimer des
> données encore servies. Preuve brute :
> [`evidence/mvp0b-daemon-gc-2026-11-08.json`](../../evidence/mvp0b-daemon-gc-2026-11-08.json).

### 12.1 Politique (`GcPolicy`, sérialisable, `deny_unknown_fields`)

| Champ | Défaut | Rôle |
|---|---|---|
| `max_cache_bytes` | `null` | plafond du cache de verdicts (`<work>/cache/*.json`) |
| `max_snapshot_bytes` | `null` | plafond du store de snapshots immuables |
| `max_incremental_bytes` | `null` | plafond de l'état incrémental par contexte (`incremental/` + `out/` + `units/`) |
| `max_total_bytes` | `null` | plafond global des trois catégories |
| `keep_recent` | `8` | nombre de contenus récents **toujours conservés** (≥ 1) |
| `dry_run` | `false` | calcule le plan et le rapporte, ne supprime rien |

Validation fail-closed : `keep_recent = 0` est refusé (il autoriserait la
suppression du contenu courant), un plafond à `0` est refusé, un champ inconnu
est refusé. Une politique sans aucun plafond est légale : elle ne supprime rien
(elle protège seulement), et le rapport l'indique.

Contenu jamais supprimé : les `keep_recent` verdicts les plus récents, le dernier
verdict servi (`last_cache_key`), le dernier snapshot analysé
(`last_snapshot_id`), tout snapshot référencé par un verdict conservé, le dernier
contexte de build utilisé, et — uniquement en l'absence d'indice de récence — le
contexte le plus récemment modifié. Les raisons sont dans
`report.kept.reasons` (ex. `keep_recent`, `last_snapshot_id`,
`referenced_by_kept_verdict`, `last_used_context`).

### 12.2 API et accès

```rust
// lib : même verrou que les analyses (lecture = analyse, écriture = GC)
let report = daemon.gc(&GcPolicy { max_cache_bytes: Some(16 * 1024), keep_recent: 4, ..Default::default() })?;
```

```powershell
# serveur JSON-lines (réponse par ligne, comme analyze/stats)
{"cmd":"gc","policy":{"max_cache_bytes":16384,"max_snapshot_bytes":131072,"keep_recent":4}}

# one-shot (aucun daemon ne doit écrire le même work_dir en parallèle)
ravum-daemon --gc --work-dir target/ravum-daemon --policy '{"max_cache_bytes":16384,"keep_recent":4,"dry_run":true}'
ravum-daemon --gc --work-dir target/ravum-daemon --max-cache-bytes 16384 --keep-recent 4
ravum-daemon --help          # documente --gc/--policy/--max-*-bytes/--keep-recent/--dry-run
```

Codes de sortie du mode one-shot : `0` succès (un rapport JSON sur stdout),
`1` politique invalide ou erreur GC, `2` erreur d'usage. Les indices de récence
sont persistés dans `<work>/retention.json` après chaque analyse servie : un GC
one-shot lancé après l'arrêt du daemon protège donc encore le contenu récent. Le
`GcReport` contient : `before`/`after` par catégorie (octets + entrées),
`planned`/`deleted` (clés, snapshots, contextes, fichiers, répertoires),
`kept.recent` (révision + `snapshot_id` conservés), `kept.reasons`,
`budgets.{cache,snapshots,incremental,total}` (`cap`, avant, après projeté,
`met`), `freed_bytes`, `planned_freed_bytes`, `duration_ms`, `dry_run`, `mode`,
`errors`, `notes`.

Sûreté : le GC prend le verrou I/O en **écriture** (les analyses le prennent en
lecture pendant toute leur durée) ; suppression en deux temps (renommage
`.gc-trash-*` puis suppression) ; répertoires absents/partiels tolérés ; une
entrée de cache illisible est traitée comme la plus ancienne candidate ; l'état
incrémental n'est collecté que **par contexte entier** (les fichiers internes de
rustc ne sont jamais compactés).

### 12.3 Mesure réelle (40 éditions, 3 contextes, machine de référence)

Campagne : `cargo run -q -p ravum-daemon --bin ravum-daemon-bench -- --suite gc`
(charge : début CPU 26,0 %, 3 310 Mo libres, 284 processus, calibration
`rustc -vV` p50 = 78 ms ; fin CPU 31,1 %, 3 295 Mo libres, 284 processus,
calibration p50 = 72 ms). Fixture `unit` (230 lignes). Accumulation : **40
éditions réelles distinctes** (40 `rustc`, tous misses) + **10 compilations sous
2 contextes supplémentaires** = 50 `rustc` en **8 048 ms** (dont 7 728 ms de
`rustc`) ; 54 requêtes au total, aucune erreur.

| Étape | Durée | Avant | Après | Libéré |
|---|---|---|---|---|
| Audit sans plafond (`dry_run`) | — | cache 39 916 B/50 · snapshots 364 555 B/50 · incrémental 5 021 563 B/3 ctx · **5 426 034 B** | idem | 0 |
| **GC #1** `max_cache_bytes=16 384`, `max_snapshot_bytes=131 072`, `keep_recent=4` | **86 ms** | cache 39 916 B/50 · snapshots 364 555 B/50 | cache **15 829 B/16** · snapshots **130 653 B/18** | **257 989 B** (34 verdicts + 32 snapshots, 194 fichiers/32 répertoires) |
| **GC #2** `max_incremental_bytes=1`, `keep_recent=4` | **26 ms** | incrémental 5 005 765 B/3 ctx | incrémental **1 667 805 B/1 ctx** | **3 337 960 B** (2 contextes âgés collectés, contexte courant conservé) |
| Dry-run final (plafonds 1 B partout) | **8 ms** | plan 1 798 206 B | disque inchangé, `freed_bytes=0` | — |

Bilan disque mesuré sur le répertoire complet : **5 440 609 → 1 827 109 B
(−66,4 %)** après les deux GC (112 ms cumulés). Après les vérifications qui
réécrivent légitimement de l'état (cache + contexte reconstruit), le répertoire
finit à 2 674 389 B, soit **−50,8 %** par rapport à l'état pré-GC (le second
chiffre est le plus conservateur). L'écart de ~9,7 Ko entre `freed_bytes` du
rapport et la baisse du répertoire sur GC #1 vient du fichier `retention.json`
(indices de récence, réécrit après chaque analyse) et des métadonnées : les deux
mesures sont reportées, aucune n'est masquée.

Budgets observés : GC #1 satisfait ses deux plafonds (`cache.met=true`,
`snapshots.met=true`) tout en gardant 4 contenus récents ; GC #2 **ne peut pas**
satisfaire `max_incremental_bytes=1` parce que le contexte actif (1 667 805 B)
est protégé → `incremental.met=false`, explicitement rapporté.

Vérifications après GC (le point critique : aucun gain obtenu par invalidation) :

| Vérification | Résultat mesuré |
|---|---|
| Contenu récent (dernier servi) analysé après GC #1 | **cache hit**, `analysis_ms = 0`, **aucun `rustc`** (1–2 ms) |
| Contenu le plus ancien (verdict collecté) analysé après GC #1 | **miss**, reconstruction par un vrai `rustc` : 185–194 ms, `rustc_runs` 50 → 51 |
| Nouveau contenu sous un contexte collecté par GC #2 | **miss**, compilé en 173 ms, répertoire incrémental du contexte recréé |
| Dry-run final | plan non vide, **0 octet libéré**, disque inchangé |
| Stats cohérentes | `gc_runs=4`, `gc_freed_bytes=3 595 949` (= somme des GC réels), `gc_deleted_entries=68` (34 + 32 + 2) |

### 12.4 Tests

`cargo test -p ravum-daemon --offline -- --test-threads=1` → **68 tests, 68
réussis, 0 échec** : les **50 tests historiques restent verts**, +9 tests
unitaires GC (politique, round-trip JSON, protections, dry-run, idempotence,
répertoires partiels, plafond global, contenu dernier) et +9 tests d'intégration
GC réels (cache hit récent / reconstruction ancienne, contenu courant jamais
supprimé, idempotence, dry-run sans suppression, liste vide/partielle, politiques
invalides, budget mémoire/processus, cohérence stats↔disque, **CLI one-shot avec
codes 0/1**, **commande serveur `gc`**). Démonstration reproductible :
`powershell -ExecutionPolicy Bypass -File scripts/demo-daemon-gc.ps1` (sortie 0
observée : dry-run planifie 2 victimes, GC réel 3→2 entrées / 1 042 B libérés,
redémarrage : récent = hit disque 0 ms, ancien = miss reconstruit 171 ms).

### 12.5 Limites (spécifiques au GC)

- **Pas de GC automatique** : aucune collecte en tâche de fond ni pendant une
  analyse ; un client ou un opérateur doit l'appeler (serveur ou one-shot).
- **Pas de compaction de l'état incrémental de rustc lui-même** : seule la
  suppression d'un contexte entier est implémentée. Avec un seul contexte actif,
  ses octets ne peuvent pas baisser ; le plafond est alors rapporté non satisfait
  (`met=false`), jamais contourné en supprimant le contexte courant.
- **One-shot et daemon vivant** : le GC one-shot suppose qu'aucun daemon n'écrit
  le même `work_dir` ; le GC concurrent multi-processus n'est pas testé.
- **Récence** : en mode one-shot, la protection repose sur `retention.json`
  persisté et, à défaut, sur les horodatages de fichiers (granularité seconde,
  ordre plus grossier). Aucun cas de fichier d'indices corrompu n'est mesuré
  au-delà de « fichier ignoré ».
- **Pas de soak test** : la mesure couvre 40 éditions (~5,4 Mo) et non plusieurs
  heures de croissance ; le coût du GC est dominé par les scans de répertoires
  (86 ms pour 50 verdicts + 50 snapshots + 3 contextes ; 26 ms pour 3 contextes),
  donc il croît avec le nombre de fichiers, pas avec la taille d'unité.
- **Pas de mesure multi-crates** : les contextes multiples sont simulés par des
  `crate_name` distincts sur la même fixture. L'état incrémental d'un vrai crate
  large (24,6 Mo mesurés en §5) n'a pas été collecté dans cette campagne.
- **Pas de GC dans la CI** : la démonstration n'est pas branchée au workflow
  GitHub (hors périmètre de cette tâche) ; elle est reproductible localement.
- **Windows uniquement** pour la campagne (métadonnées de fichiers) ; le code
  compile et les chemins sont portables, mais aucun autre OS n'est mesuré.
