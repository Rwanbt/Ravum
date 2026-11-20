# V2-L2-ir-corpus — Corpus IR/JIT et structs non `Copy`

| Champ | Valeur |
|---|---|
| Gate | T14 (ADR-010), avec test du backend natif T15 (ADR-018) |
| Branche / SHA de départ / SHA final | `v2/L2-ir-corpus` / `5b5f0755a82a1dc41c3ef32e1379766164aa65cb` / voir le commit qui ajoute ce rapport |
| Outils / OS / matériel | Cargo 1.95.0, Cranelift 0.116.1, Linux 7.0.0-43-generic x86_64, 16 CPU logiques disponibles |
| rustc | `1.95.0 (59807616e 2026-04-14)` |

## Objectif et périmètre

Mesurer la couverture du corpus de référence et exécuter des structs à champs
`String` et `Vec` dans l’interpréteur et le JIT. Le travail couvre P0-C, P0-D,
INV-05 et INV-12. Les ADR-010 et ADR-018 restent `PROPOSED`.

## Modification (fichiers)

- `crates/ravum-lang-ir/src/lower.rs` accepte les structs non génériques dont
  les champs sont représentables. Le stockage agrégé transfère les descripteurs
  du tas feuille par feuille. Le checker reste responsable de la validité des
  déplacements. Un emprunt vers un champ du tas charge le descripteur.
  Un déréférencement explicite qui traiterait ce descripteur comme une adresse
  mémoire est refusé avant la génération de code.
- `crates/ravum-lang-ir/tests/oracle.rs` vérifie les motifs des refus du corpus
  pour `u128` et `sin`.
- `crates/ravum-lang-jit/tests/owned_structs.rs` ajoute les cas différentiels
  et négatifs des structs propriétaires.
- `crates/ravum-core-machine/corpus/x-diverge.rv` préserve la boucle et
  rend le programme bien typé dans Core 1.0. `tests/oracle.rs` et
  `tests/differential.rs` exigent la divergence et rejettent les nouveaux
  cas non exécutés.

## Tests positifs / négatifs (noms, résultats)

| Test | Observation |
|---|---|
| `owned_fields_survive_moves_returns_and_mutable_borrows` | 12 programmes générés, 0 divergence. Struct imbriquée, passage et retour par valeur, mutation par emprunt, lecture de champs. |
| `reusing_a_moved_struct_is_rejected_before_lowering` | Refus du checker `E-OWN-USE-AFTER-MOVE`. |
| `replacing_an_object_through_a_heap_borrow_is_refused` | Refus explicite `Unsupported` avant le JIT. |
| `corpus_limits_are_explicit_lowering_refusals` | `u128` et `sin` donnent chacun un motif explicite. |
| `a_real_time_entry_that_allocates_is_refused` | Refus temps réel observé pour une entrée avec allocation (INV-05). |
| `native_code_agrees_with_interpreter_on_generated_programs` | 1 805 programmes arithmétiques dont 131 traps, 0 divergence. |
| `native_code_agrees_with_interpreter_on_aggregate_programs` | 600 programmes dont 340 traps, 0 divergence IR/JIT. Les 300 cas prévus pour la machine indépendante concordent aussi. |

## Commandes exécutées (exactes)

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p ravum-lang-ir -p ravum-lang-jit -- --nocapture
cargo test --workspace
cargo test --workspace --no-fail-fast
cargo build -p ravum-lang-cli --bin ravum-lang
cargo run -p ravum-conformance --release
cargo run -p ravum-conformance --release -- run
```

La comparaison indépendante lance `target/debug/ravum-lang run` sans option
de backend dans un paquet temporaire Core 1.0. Le programme est repris du
gabarit de `owned_fields_survive_moves_returns_and_mutable_borrows`. Les
quatre valeurs `-17, 0, 1, 61` sont croisées avec les trois libellés
`"", "ravum", "été"`. Chaque sortie est comparée octet pour octet à la même
attente que le test IR/JIT.

## Résultats mesurés

Le corpus de 61 programmes donne 55 accords exacts IR sur des valeurs,
et 57 programmes comparés entre l’IR et le JIT, avec 0 divergence selon les
assertions des oracles. Les deux cas exécutés hors du compteur des valeurs sont `x-stack-limit`
(faute attendue) et `x-diverge` (épuisement du budget). Le SHA de départ
n’exécutait que 56 cas. Les oracles refusent tout nouvel échec de compilation
hors des quatre cas documentés.

| Programme restant | Construction ou résultat |
|---|---|
| `o3-u128-max` | Arithmétique `u128` hors du sous-ensemble IR documenté. Refus conservé. |
| `x-libm-refused` | `std::math::sin` hors du cœur vérifiable selon MACH-5.5. Refus conservé. |
| `x-or-pattern-refused` | Motif alternatif. Refus attendu `E-UNSUPPORTED-TOKEN`. |
| `x-range-pattern-refused` | Motif d’intervalle. Refus attendu `E-UNSUPPORTED-RANGE-PAT`. |
| `x-diverge` | Boucle infinie avec `wrapping_add`. Un retour inatteignable respecte le type statique de la boucle. Les deux moteurs signalent l’épuisement du budget attendu. |
| `x-stack-limit` | Récursion profonde. Cas exécuté, faute attendue, non comptée parmi les accords sur les valeurs. |

Les 12 sorties du nouveau différentiel concordent aussi avec la voie `rustc`.
La conformance finale avec loopback donne 742 cas, dont 740 réussis,
0 échec et 2 ignorés sous Linux (`security/dependency-absolute-windows`,
`security/reserved-windows-name`).

## Limites, écarts, demandes hors périmètre

- Le déréférencement explicite d’un emprunt de tas, dont le remplacement
  de l’objet entier via `*emprunt = ...`, reste hors du sous-ensemble IR.
  Les méthodes sur l’objet emprunté sont utilisables. Le tas est libéré en
  fin d’exécution. Le test ne porte pas sur les structs génériques ou les
  vecteurs d’agrégats.
- Le premier `cargo test --workspace` échouait dans
  `ravum-conformance::stand_in_toolchain_reports_pass_fail_and_skip_honestly`.
  Le champ JSON `toolchain` expose le chemin absolu du binaire. Ce test
  utilise le harnais lui-même comme outil factice et le crate n’a aucune
  dépendance IR/JIT. Le correctif est détaillé ci-dessous.
- Le plan omet le sous-programme `run` du harnais. L’invocation littérale
  sort avec le code 2. La conformance a été exécutée avec `-- run`. Les
  cinq erreurs réseau dans le bac à sable ont disparu avec l’accès loopback.

## État des preuves

`TESTED` pour les tests ciblés, les comparaisons d’exécution et la conformance
locale. L’égalité observée porte sur les entrées listées et les sorties des
moteurs, avec le checker, le tas d’exécution et Cranelift dans la chaîne
testée. Deux cas de conformance sont `SKIP`.
La dimension `--verify-build` est `NOT_RUN`. Aucun benchmark de performance.

## Proposition à l’intégrateur

Proposer la validation de T14 sur la base des critères L2. Les quatre
refus restent explicites et justifiés. La décision de gate relève de
l’intégrateur.

## Vérification du correctif de conformance sur un checkout Linux natif

Le correctif du harnais est publié sur `codex/conformance-toolchain-path`
au SHA `64efd691b2b18fd2e5cb5e86fed98a63ab0e8891`. Il affiche le nom du
binaire dans le rapport et garde son chemin pour l’exécution. Les 57 tests
du harnais réussissent, dont le test d’origine sur le masquage. Le commit
`b3173fcd3bb437a0c8c364c6f43fe12a34bdb7a7` applique aussi ce correctif
sur L2.

Les anciens binaires de `ravum-unit-graph` contenaient un chemin de checkout
obsolète. Une reconstruction avec un répertoire de build neuf a rétabli les
tests. Les 15 tests de publication Git réussissent sur un système de fichiers
Linux natif. La campagne DSP termine avec 9 réussites en 18,30 s lorsque
seule la dépendance existante `sha2` est optimisée. Elle traite des artefacts
d’environ 4 Mio à chaque préparation. Le seuil et les assertions restent
identiques. La conformance combinée donne 740 réussites, 0 échec et 2 `SKIP`.
Le workspace termine avec le code 0, 2 324 tests réussis,
0 échec et 14 ignorés. Ce résultat s’applique aux sources du SHA L2
`b3173fcd3bb437a0c8c364c6f43fe12a34bdb7a7` avec la configuration ci-dessous.
Pour reproduire la validation, utiliser un système de fichiers Linux natif et
un répertoire de build neuf.

```sh
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo --config 'profile.test.package.sha2.opt-level=3' \
  test --workspace --no-fail-fast -- --test-threads=1
cargo run -p ravum-conformance --release -- run
```
