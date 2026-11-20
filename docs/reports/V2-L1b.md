# V2-L1b — T17 : profil `det`, étapes 1, 2 et 3

| Champ | Valeur |
|---|---|
| Gate | T17 |
| RFC | `RFC-DETERMINISTIC-PROFILE-1` (`ACCEPTED`, V2-R1) |
| Branche | `claude/trusting-cerf-4hhv5a` (pas `v2/L1b` : seule branche poussable par cette session) |
| SHA de départ | `c9d95b6` |
| Environnement | conteneur cloud Linux x86_64, rustc 1.95.0 |

## Fait (étape 1 du plan : DET-1/DET-2 statiques)

- `Profile::Det` (`check --profile det`) dans `ravum-lang-sema` :
  effets interdits au point d'entrée, transitivement : `fs net unsafe ffi` ;
  **appels** interdits par nom (`Profile::forbidden_calls`) :
  `std::io::read_line`, `std::env::var` (lectures d'entrée non rejouables) ;
  permis : `thread`, `async`, `alloc`, `panic`, l'écriture `io`, `env::args`
  (les arguments sont fixés par la commande).
- Diagnostic dédié pour un appel refusé par nom : `profile \`det\` forbids
  reading an input it cannot replay (call to \`std::io::read_line\`) …
  (DET-1)` (l'effet `io` lui-même n'est pas interdit).
- Spécification : `effects.md` (ligne `det` du tableau des profils, règle
  **EFF-4.11**) ; `features.md` : **F-EFF-011** (`experimental`, ajouté à
  FEAT-12.5) ; snapshots `rule_ids`/`feature_ids` complétés (ajout seul).
- Aide et message d'erreur du CLI : `--profile core|pure|rt|det`.

## Tests

- Conformance (5 cas, préfixe `l1b-`) : `det` accepte threads + canaux +
  `println` ; refuse `fs`, `net`, `io::read_line` (transitif
  `main -> helper`), `env::var`. Contrôle : les mêmes programmes sont
  acceptés sous `core`.
- Unitaires : `det_forbids_inputs_it_cannot_replay`, `profile_names_round_trip`.
- Corpus complet : 750 cas, 748 PASS, 0 FAIL, 2 SKIP ; `ravum-lang-sema` +
  `ravum-lang-cli` : 1 303 tests, 0 échec ; `tools/tests` 90/90.

## Étape 2 : `run --profile det --seed N`

- Runtime généré (`crates/ravum-lang-codegen/src/prelude.rs`, module
  `rv_det`) : actif seulement si le processus reçoit `RAVUM_DET_SEED`
  (positionné par `run --profile det`, retiré sinon). Un seul thread Ravum
  à la fois (`Mutex` + `Condvar`) ; à chaque `spawn`, `send`, `recv`,
  `join`, fin de thread, le suivant est tiré parmi les prêts par SplitMix64
  (arithmétique modulo 2^64 faite en `u128` puis tronquée : aucun
  `wrapping_*`, les tests d'audit du prélude restent verts) ; `recv` vide et
  `join` non fini attendent un événement (envoi, `Sender` détruit, fin de
  thread) ; aucun thread prêt alors que certains attendent :
  `panic: deadlock (det, seed=N)`, code 1. Aucun `unsafe`.
- CLI : `run --profile det [--seed N]` (graine 0 par défaut) ; refus
  d'usage (code 2) : `--seed` sans `det`, `run --profile pure|rt|core`,
  graine non numérique, `det` en wasm. Le programme est vérifié sous `det`
  (DET-1) avant de tourner. Ligne `profile det: seed=N` sur stderr.
- Tests `crates/ravum-lang-cli/tests/det_run.rs` (3) : 12 graines × 5
  exécutions, même graine ⇒ même sortie (DET-T1), au moins deux ordres
  observés (DET-T2, mesuré : ≥ 2 sur 12 ; à la main, 5 ordres sur 7
  graines), toutes les valeurs une fois ; interblocage arrêté avec sa graine
  (DET-T4, 0,6 s mesuré) ; refus `read_line` et arguments invalides ; sans
  `det`, exécution inchangée.

## Pas fait, dit clairement

- **DET-2** (`nondet` produit) : aucune fonction de la stdlib ne lit
  aujourd'hui une horloge ni un aléa ; il n'y a donc rien à marquer.
  `task::timeout` (RFC-ASYNC-NET-1) sera la première source.
- ~~DET-5~~ : **fait** — `test --profile det [--seeds N]` (N de 1 à 10 000,
  défaut 1) vérifie sous `det` puis exécute le lanceur de tests pour les
  graines `0..N` ; la première graine en échec est nommée avec la commande
  qui la reproduit (`--seeds k+1`). Test DET-T3 (`det_run.rs`) : un test
  dépendant de l'ordre échoue pour une graine donnée, la commande indiquée
  le reproduit 3 fois sur 3, les graines précédentes passent.
- **Horloge virtuelle** de l'exécuteur `std::task` : non faite ; sans effet
  observable aujourd'hui (le sommeil de 1 ms n'a lieu qu'en attente de
  socket, et `net` est refusé sous `det`). À faire avec `task::timeout` (L7b).
- **Étape 3, faite pour `fs`, `io::read_line`, `env::var`** :
  `run --profile det [--seed N] --record F` / `--replay F`. Runtime
  (`rv_rec`, prélude) : en enregistrement, chaque appel s'exécute puis ajoute
  `rang \t nom \t empreinte(args) \t résultat` au journal ; en rejeu, aucun
  appel externe : rang, nom et empreinte (FNV-1a 64, détection et non
  authentification) doivent correspondre, sinon `panic: replay diverged at
  #n` (aussi journal épuisé) ; une écriture rejouée n'est pas faite (DET-12).
  CLI : en-tête `ravum-rvlog-1 seed= binary=sha256: platform=` (DET-13) :
  journal d'un autre binaire ou d'une autre plateforme refusé avant
  exécution (code 2) ; graine lue dans le journal (`--seed` avec `--replay` :
  code 2) ; avertissement « contient les données lues » (DET-14).
  Vérification : profil interne `DetIo` (affiché `det`) qui permet ces
  entrées journalisées et refuse toujours `net unsafe ffi nondet`.
  Test `record_then_replay_serves_the_recorded_inputs_and_performs_no_io` :
  entrée modifiée et variable absente au rejeu → même sortie, fichier non
  écrit ; empreinte altérée → divergence #1 ; journal tronqué → fin de
  journal ; en-tête étranger → code 2 ; usages invalides → code 2.
- **Pas fait de l'étape 3** : `net` (sockets, HTTP) et l'horloge
  (`task::timeout`) ne sont pas journalisés ; ils restent refusés sous `det`
  (DET-T8 avec un serveur HTTP : non fait).
- Déterminisme **entre plateformes** non mesuré (une seule : Linux x86_64).

## État des preuves

`TESTED` (refus statique ; même graine ⇒ même sortie sur le corpus des tests, Linux). Pas de record/replay. T17 reste `PARTIAL`.
