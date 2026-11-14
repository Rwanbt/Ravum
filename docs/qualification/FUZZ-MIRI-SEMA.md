# Qualification fuzz + Miri — frontend sémantique Ravum (`syntax` + `sema`)

> **Statut : PARTIAL (mesures réelles, couverture partielle assumée).**
> Aucune preuve de sûreté générale n'est revendiquée. Les chiffres ci-dessous
> sont reproductibles par les commandes indiquées, sur la machine et les
> toolchains listés. Ce document ne ratifie aucun ADR.

## 1. Objet et périmètre

Cette qualification couvre **uniquement** :

- le frontend Ravum : `ravum-lang-syntax` (lexer, parser, AST, formatter) et
  `ravum-lang-sema` (résolution, typage, propriété, effets, profils) ;
- un fuzz de robustesse/déterminisme du couple parse → check, via le nouveau
  test `crates/ravum-lang-sema/tests/fuzz_check.rs` ;
- l'exécution Miri des tests unitaires/integration de ces deux crates.

Hors périmètre (non exécuté, non revendiqué) : `ravum-lang-cli` (édition en
cours par un autre worker au moment de la mesure), `ravum-lang-codegen`,
`tools/conformance`, la conformance E2E, et toute propriété de sûreté du code
généré.

Fichiers créés par cette mission (aucun `src/`, test existant ni `Cargo.toml`
n'a été modifié ; aucune mutation git) :

- `crates/ravum-lang-sema/tests/fuzz_check.rs`
- `docs/qualification/FUZZ-MIRI-SEMA.md` (ce document)

## 2. Environnement

| Élément | Valeur |
|---|---|
| Dépôt | `D:\App\Ravum` |
| Branche | `feat/lang-core-1.0-rc` |
| Commit mesuré | `8fd1547` |
| Date des mesures | 2026-11-09, ~17:20–17:35 Europe/Paris |
| Stable | `rustc 1.95.0 (59807616e 2026-04-14)` / cargo 1.95.0 |
| Nightly (Miri) | `rustc 1.97.0-nightly (365c0e1d7 2026-05-06)` |
| Miri | `miri 0.1.0 (365c0e1d7a 2026-05-06)` |
| OS | Windows x86_64 (hôte des mesures) |
| RAM | 16 Go (référence projet) |

## 3. Fuzz du frontend (`fuzz_check.rs`)### 3.1 Générateur

- Générateur **xorshift64\*** déterministe, graines fixes :
  - `fuzz_check_short` : graine de base `0x0052_4156_554D_465A` ;
  - `fuzz_check_long` : graine de base `0x0052_4156_554D_465A_4C`.
- Corpus de 8 programmes couvrant : hello/io, structs/enums/impl/match,
  récursion + `while` + `Result`, effets déclarés + `?`, modules inline +
  chemins, génériques + `Vec`/`Option`/tableaux/index/casts, Unicode dans les
  littéraux, tuples/boucles `for`/`loop`.
- Mutations (1 à 5 par cas) : insertion/suppression/remplacement de fragments
  (mots-clés, identifiants, ponctuation, littéraux), duplication de plages,
  troncature, bruit ASCII. Taille bornée à 8 192 octets.
- Profil cyclé sur `core` / `pure` / `rt` pour exercer la vérification d'effets.

### 3.2 Invariants vérifiés à chaque itération

Pour chaque programme muté (parse + check complets, deux fois) :

1. **Aucun panic** : parse et check sont exécutés sous `catch_unwind` ; un
   panic produit un échec avec graine, profil, source et hexadécimal.
2. **Diagnostics bien formés** : code non vide, message non vide, notes non
   vides ; chaque span (primaire ou de note) est soit `Span::dummy()`, soit
   contenu dans le fichier analysé (`file == 0`, `start ≤ end ≤ len`).
3. **Déterminisme strict** : deux exécutions indépendantes de la même source
   produisent exactement les mêmes diagnostics **et** les mêmes faits
   (`expr_types`, `pat_types`, `type_nodes`, `item_paths`, `effects`, `calls`,
   `resolutions`, `methods`, `main_fn`, `test_fns`).
4. **Reproduction** : à l'échec, le panic imprime `seed`, `iteration`, `profile`,
   `source_len`, `source_hex` (hexadécimal des octets) et la source en clair.

### 3.3 Budgets et commandes

Le budget se règle par `RAVUM_FUZZ_ITERS` (défaut : 300 pour le test court,
20 000 pour le test long `#[ignore]`).

```powershell
# Court (300 itérations par défaut)
cargo test -p ravum-lang-sema --test fuzz_check fuzz_check_short -- --nocapture

# Long (20 000 par défaut, mode ignoré)
$env:RAVUM_FUZZ_ITERS='20000'
cargo test -p ravum-lang-sema --test fuzz_check fuzz_check_long -- --ignored --nocapture
Remove-Item Env:RAVUM_FUZZ_ITERS

# Extension (facultatif, même test long, budget élargi)
$env:RAVUM_FUZZ_ITERS='100000'
cargo test -p ravum-lang-sema --test fuzz_check fuzz_check_long -- --ignored --nocapture
Remove-Item Env:RAVUM_FUZZ_ITERS
```

### 3.4 Résultats mesurés

| Run | Itérations | Durée mesurée | Débit | Verdict |
|---|---:|---:|---:|---|
| Court (défaut) | 300 | 67,3 ms | ≈ 4 460/s | **PASS** |
| Long (défaut) | 20 000 | 5 114 ms | 3 911/s | **PASS** |
| Extension | 100 000 | 25 800 ms | 3 876/s | **PASS** |

- **Aucun panic, aucun diagnostic mal formé, aucune non-déterminisme** sur
  l'ensemble des runs (120 300 itérations cumulées, 2 exécutions par itération).
  Mesures finales refaites après les dernières éditions de `src/` (mtimes
  ≤ 2026-11-09 17:30:24) ; les runs Miri et fuzz ci-dessous datent de 17:36–17:43.
- Aucun bug frontend n'a été trouvé par ce fuzz (voir §6 pour les limites).

### 3.1 Rejeu après l'implémentation des traps `rt` (2026-11-14)

La campagne de 100 000 itérations a été **rejouée après** l'ajout de la passe
`traps.rs` (RFC-RT-TRAPS-1, round 4). Le générateur choisit le profil `rt` dans
un cas sur trois (voir `fuzz_round`), donc environ 33 000 exécutions ont exercé
la nouvelle passe de détection de traps, y compris ses formes d'AST nouvelles.

| Run | Itérations | Durée | Débit | Verdict |
|---|---:|---:|---:|---|
| Long post-traps | 100 000 | 18 957 ms | 5 275/s | **PASS** |

Commande :

```
$env:RAVUM_FUZZ_ITERS='100000'
cargo test -p ravum-lang-sema --test fuzz_check fuzz_check_long -- --ignored --nocapture
```

Aucun panic, aucun diagnostic non déterministe, aucun span invalide. Le débit
plus élevé que le run initial (3 876/s → 5 275/s) reflète une machine moins
chargée, pas un changement de sémantique.

## 4. Miri

### 4.1 Commande de référence

```powershell
$env:MIRIFLAGS='-Zmiri-disable-isolation'
cargo +nightly miri test -p ravum-lang-syntax -p ravum-lang-sema --lib --tests
```

Cette commande « brute » n'est **pas** exécutable de bout en bout sur l'hôte
Windows à cause des limitations de Miri (système de fichiers Windows et tests
sensibles au temps), d'où les exclusions explicites ci-dessous. Le périmètre
réellement exécuté reste intégralement dans `syntax` + `sema`.

### 4.2 Exclusions et raisons (liste exacte)

Aucun test de `ravum-lang-syntax` ni de `ravum-lang-sema` ne lance de
sous-processus (`std::process::Command` absent de ces crates) : les exclusions
ne sont donc **pas** des exclusions de sous-processus, mais :

| Binaire / test exclu | Raison |
|---|---|
| `ravum-lang-syntax --test parse_examples` (3 des 4 tests ; seul `empty_and_comment_only_files_parse` s'exécute sans fs) | `std::fs::read_dir` non supporté par Miri sur l'hôte Windows : `unsupported operation: can't call foreign function FindFirstFileExW on OS windows`. Ce n'est pas un défaut du code. |
| `syntax --lib` → `hostile_100k_tokens_parses_under_two_seconds` | assertion de temps (< 2 s) inapplicable sous Miri (interpréteur ~100× plus lent) ; entrée de ~100 k tokens. |
| `syntax --lib` → `hostile_deep_nesting_reports_depth_without_panic` | assertion de temps (< 2 s) inapplicable sous Miri. |
| `syntax --lib` → `too_large_token_stream_is_reported_without_parsing` | construit `MAX_TOKENS + 10` = ~300 010 tokens : coût disproportionné sous Miri (budget temps). |
| `syntax --lib` → `examples_are_idempotent_and_meaning_preserving` | parcours du répertoire `examples/` via `read_dir` (non supporté, cf. ci-dessus). |
| `sema --lib` → tous les tests `loader::` (40 tests) | chargement de packages : `read_dir`, `canonicalize`, `symlink_metadata` sur arborescences temporaires ; système de fichiers Windows non supporté par Miri. |
| `sema --test fuzz_check` → `fuzz_check_long` | mode long `#[ignore]` non exécuté sous Miri (budget temps) ; le mode court est exécuté avec un budget réduit (3 itérations). |

Note : `syntax --lib` → `hello_example_is_already_canonically_formatted` a bien
été exécuté sous Miri (lecture d'un fichier précis via `read_to_string`),
contrairement au parcours de répertoire.

### 4.3 Périmètre exact commandé et résultats

Toutes les exécutions avec `MIRIFLAGS='-Zmiri-disable-isolation'`.

| Phase | Commande | Tests | Durée | Verdict |
|---|---|---|---:|---|
| 1. `syntax` lib | `cargo +nightly miri test -p ravum-lang-syntax --lib -- --skip hostile_100k_tokens --skip hostile_deep_nesting --skip too_large_token_stream --skip examples_are_idempotent_and_meaning_preserving` | **65 passés**, 0 échec, 4 filtrés | 206,6 s (3 min 27 s) | **PASS** |
| 2. `sema` lib | `cargo +nightly miri test -p ravum-lang-sema --lib -- --skip loader::` | **87 passés**, 0 échec, 40 filtrés | 121,5 s (2 min 02 s) | **PASS** |
| 3. `sema` fuzz (mode court) | `RAVUM_FUZZ_ITERS=3 cargo +nightly miri test -p ravum-lang-sema --test fuzz_check -- --nocapture` | **1 passé** (3 itérations), 1 ignoré (`fuzz_check_long`) | 17,9 s (mur : 21,3 s) | **PASS** |
| 4. `syntax` `parse_examples` | `cargo +nightly miri test -p ravum-lang-syntax --test parse_examples` | **1 passé / 3 échecs environnementaux** (cf. §4.2) | 38,3 s | **NOT_RUN (environment)** |

**Total Miri mesuré : ≈ 5 min 53 s** (3:27 + 2:02 + 0:21), sous le plafond de
15 min. Aucune erreur mémoire (UB, accès invalide, fuite signalée par Miri)
dans les tests exécutés.

## 5. Reproductibilité

- Fuzz : graines fixes dans le fichier de test ; budget par `RAVUM_FUZZ_ITERS`.
  Un échec imprime `seed`, `iteration`, `profile`, la source et son hexadécimal.
  Rejouer une graine : copier la source hexadécimale dans un fichier et appeler
  `ravum_lang_sema::check` (ou ajuster le générateur pour démarrer à la graine
  indiquée).
- Miri : les commandes exactes sont en §4.3 ; l'ajout des `--skip` est
  nécessaire sur l'hôte Windows pour les raisons listées en §4.2.
- Aucun `git commit/push/checkout/reset` n'a été exécuté.

## 6. Limites et statut honnête

- Le fuzz est **mutationnel, non guidé par la couverture** : il exerce la
  robustesse lexicale/syntaxique et les chemins d'erreur, pas une preuve
  d'absence de bug sémantique. Un oracle limité aux invariants (pas de panic,
  codes/spans valides, déterminisme) ne détecte pas un diagnostic *faux* mais
  bien formé.
- Les profils sont cyclés, mais la profondeur d'un cas est bornée (1–5
  mutations, 8 192 octets) : les gros programmes et les combinaisons profondes
  (génériques imbriqués, modules multi-fichiers) sont peu couverts.
- Miri n'a couvert **ni** les tests `loader::` (système de fichiers), **ni**
  `parse_examples` (parcours de répertoire), **ni** le mode long du fuzz ;
  l'absence d'erreur mémoire sur ce sous-ensemble **n'est pas** une preuve pour
  le reste de `sema`/`syntax`.
- Miri sur Windows ne supporte pas certains appels système de fichiers
  (`FindFirstFileExW`) ; les tests `loader` sont donc non exécutables ici même
  avec `-Zmiri-disable-isolation`, indépendamment de leur validité.
- Aucune mesure de performance du compilateur n'est revendiquée au-delà des
  débits du fuzz (machine locale, profil `dev`, `incremental = false`).
- Statut global : **PARTIAL** — le fuzz court/long/large et le périmètre Miri
  documenté passent ; la couverture n'est ni exhaustive ni une preuve.

## 7. Journal des commandes (extraits)

```text
fuzz_check_short: 300 iterations in 67.2868ms                      -> PASS
fuzz_check_long: 20000 iterations in 5114 ms (3911/s)              -> PASS
fuzz_check_long: 100000 iterations in 25800 ms (3876/s)            -> PASS
fuzz_check_long: 100000 iterations in 18957 ms (5275/s)            -> PASS (post-traps, round 12)
syntax --lib (miri): 65 passed; 0 failed; 4 filtered; 206.56s      -> PASS
sema  --lib (miri): 87 passed; 0 failed; 40 filtered; 121.50s      -> PASS
sema  fuzz  (miri, RAVUM_FUZZ_ITERS=3): 3 iterations in 17.90s     -> PASS
syntax parse_examples (miri): 1 passed / 3 environmental failures  -> NOT_RUN
```

Aucun bug frontend n'a été observé pendant cette campagne ; aucun correctif
n'a été apporté à `src/` (hors périmètre de la mission).
