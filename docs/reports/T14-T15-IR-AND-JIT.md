# T14 et T15 — IR v1 et backend natif Cranelift (2026-11-19)

> Gates **T14** (ADR-010) et **T15** (ADR-018), V1
> ([DECISIONS-2026-11-19-V1](../decision/DECISIONS-2026-11-19-V1.md)).
> Les deux passent `FAIL`/`NOT_STARTED` → **`PARTIAL`**. Aucun `PROVED`.

## Objectif

Sortir `rustc` de la boucle de développement : une IR propre au langage,
consommée par un interpréteur de référence et par un backend natif en
processus, avec une sémantique **identique** vérifiée par oracle.

## Modification (nouveaux crates, aucun crate existant modifié)

| Crate | Rôle | `unsafe` |
|---|---|---|
| `crates/ravum-lang-ir` | IR v1 (`ravum-ir-1`) : CFG de blocs sur des slots typés ; JSON versionné ; `validate()` fail-closed ; abaissement depuis `CheckedProgram` (types pris dans les faits du checker, jamais réinférés) ; interpréteur de référence | `forbid` |
| `crates/ravum-lang-jit` | Cranelift 0.116 → code machine en mémoire. Overflows vérifiés explicitement ; traps sans déroulement (code stocké dans un `Ctx`, propagé à chaque appel) ; compteur de profondeur (512) et carburant de boucles → `fault`/`diverge` au lieu d'un crash ou d'un blocage | `deny` + 1 site audité (#7 de `UNSAFE-INVENTORY.md`) |

Sous-ensemble IR v1 : entiers ≤ 64 bits, `f32`/`f64`, `bool`, `()`,
fonctions du module racine, `let`, affectations, `if`, `while`, `loop`,
`for i in a..b`, `break`/`continue`/`return`, arithmétique vérifiée, `as`,
`panic("littéral")`. Tout le reste → `Unsupported` explicite.

**Messages de trap** : alignés sur la **spec** (EXPR-3.2/3.3,
`core-machine.md` §5 : `division by zero`, `remainder by zero`,
`shift out of range`). Constat : le backend `rustc` actuel émet les messages de
Rust (`attempt to divide by zero`…) ; la conformité de ce backend à EXPR-3.2
n'est pas examinée ici (`NOT_REVIEWED`).

## Tests

| Test | Résultat |
|---|---|
| `ravum-lang-ir` `oracle::ir_interpreter_agrees_with_the_reference_machine` — 59 programmes de `ravum-core-machine` (oracle **indépendant** du compilateur) | **41 identiques, 0 divergence**, 14 `Unsupported` (méthodes, `math::*`, tableaux, `u128`, enum/match, struct, motif de `for`), 3 refusés par le checker (attendu) |
| `oracle::json_round_trip_is_lossless_and_versioned` | aller-retour JSON identique ; **négatifs** : version inconnue refusée, slot hors bornes refusé |
| `oracle::unsupported_constructs_are_refused_not_approximated` | **négatif** : struct, méthode, tuple → `Unsupported` |
| `ravum-lang-jit` `differential::native_code_agrees_with_interpreter_and_reference_machine` | 42 programmes : natif = interpréteur = machine, **0 divergence** |
| `differential::native_code_agrees_with_interpreter_on_generated_programs` — générateur déterministe (10 types entiers × 10 opérateurs × valeurs limites, casts vers 12 types, flottants saturants) | **1 805 programmes, 131 traps, 0 divergence** ; test de puissance : ≥ 500 programmes et ≥ 50 traps exigés |
| `differential::deep_recursion_and_endless_loops_are_reported_not_crashes` | **négatif** : récursion infinie → `fault: call depth limit` ; boucle infinie → `diverge` |

Tests exécutés en debug (vérificateur Cranelift **actif**) et en release.

## Mesure de build (même source, même machine)

`cargo test -p ravum-lang-jit --release --test differential jit_build_time -- --ignored --nocapture`
pour le JIT ; `ravum-lang build` (release du CLI, `.ravum`/`target` effacés, 5
exécutions) pour la voie `rustc`. Programme synthétique `synthetic(n)` du test.

| Fonctions (lignes) | voie `rustc` (`build` complet) | JIT : front (parse+check+lower) + Cranelift | Gain |
|---|---|---|---|
| 100 (1 207) | 370–394 ms | 5,7 + 11,6 ≈ **17 ms** | ≈ ×21 |
| 800 (9 607) | 725–796 ms | 66,7 + 83,9 ≈ **150 ms** | ≈ ×4,8 |

Corrections de performance trouvées en mesurant : import des seules fonctions
appelées (l'import de toutes était quadratique), bloc de sortie unique par
fonction, `opt_level=none`, allocateur `single_pass`, vérificateur désactivé
en release seulement. Limites : une seule machine, conteneur partagé 4 cœurs ;
la voie `rustc` inclut l'édition de liens et l'écriture du binaire, le JIT
non ; à 800 fonctions le front (66 ms) devient la moitié du coût — c'est là que
T13 étape 2 (vérification par item) prend son sens.

## Bloquant : politique `cargo-deny` — **décision du responsable**

Sur `dev` sans ces changements, `cargo deny check` est vert. Avec le JIT :

- **15 refus de licence** : les crates Cranelift sont sous
  `Apache-2.0 WITH LLVM-exception`, absente de `deny.toml` `[licenses].allow`
  (l'exception LLVM ne fait qu'**élargir** les droits d'Apache-2.0) ;
- **3 doublons** (`multiple-versions = "deny"`) : `hashbrown` (3 versions),
  `bitflags`, `windows-sys`, tirés par Cranelift.

Je n'ai **pas** modifié `deny.toml` (ne jamais affaiblir une validation pour
obtenir du vert). Les versions récentes de Cranelift (0.137) exigent
`rustc 1.97` (le dépôt épingle 1.95) et gardent les doublons. La CI `mvp`
(qui lance `cargo-deny`) ne tourne pas sur cette branche ; **une PR vers `dev`
serait rouge** tant que ce point n'est pas tranché.

## Commandes

```
cargo test -p ravum-lang-ir -- --nocapture
cargo test -p ravum-lang-jit -- --nocapture
cargo test -p ravum-lang-jit --release -- --nocapture
cargo clippy -p ravum-lang-ir -p ravum-lang-jit --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

## Contexte

Linux x86_64 (conteneur cloud, 4 cœurs), `rustc 1.95.0 (59807616e 2026-04-14)`,
Cranelift 0.116.1, branche `claude/trusting-cerf-4hhv5a` fusionnée avec
`origin/dev` `bb8d5a2`. État : `TESTED`.

## IR v1.1 (ajout du 2026-11-19)

Mémoire explicite : type `Ptr`, emplacements de pile par fonction,
`Load`/`Store` à décalage fixe, `PtrAdd` (index déjà vérifié). Les agrégats
`Copy` (structs, tuples, tableaux de taille fixe) vivent en mémoire et sont
**copiés** à chaque liaison, affectation, argument par valeur et retour (par
adresse de résultat) : deux variables ne partagent jamais leur stockage.
`&`/`&mut` désignent un lieu en mémoire ; une référence vers un scalaire local
est refusée. Tout index est comparé à la longueur avant que l'adresse existe.
L'interpréteur a une mémoire d'octets dont chaque accès est vérifié (un accès
hors zone est une faute, jamais une valeur).

| Oracle | Résultat |
|---|---|
| corpus `ravum-core-machine` | **44/59 identiques** (41 en v1), 0 divergence ; `struct-and-tuple`, `o3-index-out-of-bounds`, `o8-for-bounds-once` passent |
| 600 programmes agrégats générés (structs imbriquées, tableaux de structs, passage par valeur, retours d'agrégats, `&mut`, traps de bornes et de dépassement) | natif = interpréteur, **0 divergence**, 340 traps |
| dont 300 sans `&mut` comparés à la **machine indépendante** | **300/300 identiques** |
| tous les tests v1 (1 805 générés, corpus, hot swap) | inchangés |

Écart de spec relevé : EXPR-7.6 demande `index out of bounds: the len is N but
the index is i` ; la machine de référence et l'IR donnent la forme courte.

## IR v1.2 (ajout du 2026-11-19) — méthodes scalaires

`ravum-ir-1.2` ajoute des opérations dont la sémantique est **exactement**
celle d'une méthode de la stdlib (STD-12.7, RFC-RT-TRAPS) : `BinOp`
`Wrapping{Add,Sub,Mul}`, `Saturating{Add,Sub,Mul}` ; `UnOp` `Abs` (entier
signé vérifié : `attempt to negate with overflow` sur MIN ; `f64` : bit de
signe), `WrappingNeg`, `SaturatingNeg`, `Sqrt`, `Floor`, `Ceil` (`f64`).
`validate()` refuse une opération sur un type qu'elle n'accepte pas
(fail-closed : un backend ne voit jamais `Sqrt` sur un entier). Restent
`Unsupported` : `checked_*` (rend `Option`), `round` (arrondi « loin de zéro »
≠ `nearest`), `min`/`max` (règle NaN), `powf`, `to_string`, méthodes
utilisateur. Version d'IR incrémentée : un lecteur v1.1 refuse un JSON v1.2.

| Oracle (`crates/ravum-lang-jit/tests/methods.rs`) | Résultat |
|---|---|
| 10 types entiers × 12–14 valeurs limites × 6 méthodes binaires + 3 unaires signées ; `f64` : 12 valeurs (±0, ±inf, NaN, ties) × 4 | **8 058 programmes : natif = interpréteur = machine, 8 058/8 058**, 5 traps (`abs(MIN)`) ; debug (vérificateur actif) et release |
| **négatif** : `checked_add`+`match`, `round`, `min`, `powf` | `Unsupported` |
| **négatif** : IR forgée (`Sqrt` sur `I64`) | refusée par `validate`/`from_json` |
| corpus `ravum-core-machine` | **48/60 identiques** (44/59 avant), 0 divergence |

**Bug trouvé dans la machine de référence** : `1u8.saturating_sub(7)` rendait
`255` (MAX) au lieu de `0`. La spec dit « saturating » (STD-12.7) et la voie
`rustc` délègue à `u8::saturating_sub` (→ 0) ; l'IR et le JIT donnaient déjà
0. Correction d'une ligne dans `crates/ravum-core-machine/src/eval.rs` et
programme de corpus `saturating-sub-unsigned.rv` (`// oracle: value u8 0`).
140 programmes sur 8 058 étaient concernés (tous `uN`/`usize`
`saturating_sub` en dépassement bas).

Tests ajustés (aucun cas négatif retiré) : dans
`ravum-lang-ir/tests/oracle.rs` et `ravum-lang-cli/tests/watch_jit.rs`,
`wrapping_add`/`abs`, désormais couverts, sont remplacés par `round`/`min`,
toujours hors IR.

## IR v1.2, suite — `enum`, `Option`, `match`, `checked_*`

- **Disposition** : un enum `Copy` non générique (et `Option<T>`, `None` = 0,
  `Some` = 1) vit en mémoire : tag `u32` à l'offset 0, puis la charge utile de
  chaque variante disposée comme un tuple à partir du même offset. Une copie
  d'enum copie les mots alignés qui le couvrent (la charge active dépend du
  tag) ; aucune nouvelle instruction d'IR n'est nécessaire.
- **Constructeurs** : variantes unitaires, tuple, à champs nommés (champs
  évalués dans l'ordre du source, rangés dans l'ordre de déclaration),
  `Some(x)`, `None`.
- **`match`** : bras essayés dans l'ordre (O7) ; motifs `_`, liaison (par
  copie), littéraux entiers/flottants/booléens, tuples, variantes imbriquées ;
  les liaisons sont faites après le succès de tout le motif. Un `match` qui
  sortirait de tous les bras (impossible : exhaustivité prouvée par le
  checker) atteint `Unreachable` → `fault`, jamais une valeur.
- **`checked_add/sub/mul/div/rem/neg`** : nouvelles `BinOp`
  `Overflows{Add,Sub,Mul}` (booléen, sans trap) ; l'opération elle-même ne
  s'exécute que sur le chemin `Some`.
- Restent `Unsupported` : enums génériques ou non-`Copy`, `Result`, motifs
  `char`/chaîne/constante, `match` sur une référence.

| Oracle (`crates/ravum-lang-jit/tests/enums.rs`) | Résultat |
|---|---|
| `checked_*` : 10 types × valeurs limites × 5 opérations + `checked_neg` | **3 775 programmes, natif = interpréteur = machine**, 0 trap |
| enums/`match` : 5 variantes (unitaire, tuple, champs), struct contenant un enum, `Option<Shape>`, tuple de `Option` en scrutin, tableaux d'enums, passage et retour par valeur, motifs littéraux et imbriqués, traps dans un bras et dans une charge utile | **614 programmes (variantes tuple) natif = interpréteur = machine** ; **614 (variantes à champs nommés) natif = interpréteur** (la machine n'a pas de variantes à champs nommés) |
| **négatif** : une liaison de `match` est une copie (réaffecter le scrutin ensuite ne la change pas) | identique sur les 3 moteurs |
| **négatif** : enum générique, `max`, `round`, `min`, `powf` | `Unsupported` |
| corpus `ravum-core-machine` | **51/60 identiques**, 0 divergence ; restent 5 `Unsupported` (`math::*`, `u128`, motif de `for`…) et 3 refus attendus |
| debug (vérificateur Cranelift actif) et release | identiques |

Limite de la machine relevée (pas de l'IR) : dans un bras `None => -77`,
la machine type le littéral en `i32` alors que le `match` est `i64` ; l'oracle
passe par une variable typée.

## Suite proposée

1. Élargir IR v1 : struct/tuple/tableau Copy, `match`, méthodes scalaires
   (`abs`, `checked_*`, `saturating_*`, `wrapping_*`), `math::*`, `i128`.
2. `ravum-lang run --backend jit` dans le CLI (touche un crate partagé).
3. T16 : le JIT rend le hot swap naturel (table d'indirection par fonction) ;
   à articuler avec `RFC-E11-HOT-RELOAD` déjà présent côté `rustc` sur `dev`.
