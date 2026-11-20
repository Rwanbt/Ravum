# V2-L1b — T17 : profil `det`, étape 1 (refus statique)

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

## Pas fait (étapes 2 et 3), dit clairement

- **DET-2** (`nondet` produit) : aucune fonction de la stdlib ne lit
  aujourd'hui une horloge ni un aléa ; il n'y a donc rien à marquer.
  `task::timeout` (RFC-ASYNC-NET-1) sera la première source.
- **DET-3 à DET-8** (ordonnanceur et horloge virtuels, `--seed`,
  `--seeds`, interblocage) et **DET-9 à DET-15** (`--record`/`--replay`) :
  non implémentés. `run` et `test` n'acceptent pas `--profile` : aucune
  exécution n'est présentée comme déterministe. L'étape 2 touche le runtime
  de `std::thread`/`std::task` dans le Rust généré ; L7b doit attendre.

## État des preuves

`TESTED` pour le refus statique. Aucun rejeu n'existe encore. T17 reste `PARTIAL`.
