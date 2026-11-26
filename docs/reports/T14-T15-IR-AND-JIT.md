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
the index is i` ; la machine de référence et l'IR donnaient la forme courte.
**Résorbé le 2026-11-24 (V2-G8)** : `ravum-ir-1.5` ajoute le terminateur
`Term::TrapBounds { len, index }`. Il produit le texte de la spec dans
l'interpréteur et dans le JIT (via `ctx.scratch`/`scratch2`, comme les
vecteurs), n'imprime rien et reste donc admis par le validateur temps réel
(INV-05), qui n'a pas été modifié. La machine de référence donne le même
texte, et le corpus `o3-index-out-of-bounds` l'exige désormais en entier.

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

## IR v1.2, suite — `math::*` et méthodes d'`Option`

- `math::abs/sqrt/floor/ceil` (opérations existantes), `math::round` (`UnOp::Round`,
  égalité loin de zéro, STD-6.3 : `trunc` puis ±1 si `|x - trunc(x)| ≥ 0,5`,
  soustraction exacte), `math::min/max` (`BinOp::MathMin/MathMax`, STD-6.5 :
  NaN → l'autre opérande, `min(-0.0, +0.0) = -0.0`). `pow` et la libm
  (`sin`…) restent refusés (MACH-5.4). Les **méthodes** `f64::min/max/round`
  restent `Unsupported` : leur règle (Rust) n'est pas STD-6.5.
- `Option::is_some/is_none/unwrap_or` (receveur puis argument, O1) ;
  `unwrap` reste `Unsupported` (texte de panique différent entre machine et
  voie `rustc`).
- **Trou de validation corrigé** : `bin_op_accepts` ne refusait pas
  `Overflows*` sur un flottant (un remplacement de texte précédent n'avait pas
  pris après `cargo fmt`). Test négatif ajouté (IR forgée `OverflowsAdd`/f64,
  `MathMin`/i64, `WrappingMul`/f64 → refusées).

| Oracle | Résultat |
|---|---|
| `math::*` : 19 valeurs difficiles (égalités ±0,5/±1,5/±2,5, `0.49999999999999994`, `4503599627370495.5`, ±0, ±inf, NaN) × 5 fonctions + 19² paires × `min`/`max` (résultat inversé pour distinguer ±0) | **817 programmes, natif = interpréteur = machine** |
| `Option` : `is_some`/`is_none`/`unwrap_or` sur scalaires et charge tuple | 9 programmes identiques sur les 3 moteurs |
| corpus `ravum-core-machine` | **54/60 identiques**, 0 divergence ; restent `u128` et `sin` (refus voulu) |

## IR v1.2, suite — méthodes utilisateur

`impl` non générique : fonctions associées (`S::new()`), `self` (copie),
`&self`/`&mut self` (adresse du lieu, auto-référence ; une référence est
transmise telle quelle), méthodes qui appellent des méthodes, retours
d'agrégats. Nom IR d'une méthode : son chemin (`Acc::add`), donc deux types
ne partagent jamais un nom (le hot swap compte les cases par nom). `impl`
générique : `Unsupported`.

| Oracle | Résultat |
|---|---|
| 16 programmes (`&mut self` en chaîne, `self` par valeur, struct de structs, débordement et division par zéro dans une méthode) | natif = interpréteur, 4 traps (la machine n'a pas d'`impl`) |
| **oracle indépendant** `ravum-lang-cli/tests/jit_print_rustc.rs` : 15 cas par `ravum-lang run` (rustc) et par le JIT | **sortie identique octet par octet** |

## IR v1.2, suite — `const`

`const` entiers et booléens (les seuls que le checker évalue) : valeur en
expression, et motif de constante par chemin (`crate::K =>`, EXPR-5.8).

**Deuxième bug trouvé dans la machine de référence** : son parseur traitait
tout identifiant seul commençant par une majuscule comme une variante
unitaire, donc `K =>` ne correspondait jamais. La spec dit le contraire :
G-6.38 (« le pattern `IDENT` lie par valeur ») et G-6.41 (seuls `Some`,
`None`, `Ok`, `Err` sont des motifs de constructeur sans qualification). Le
checker, la voie `rustc` (vérifié : `1, 1, 1`) et le JIT lient. Correction
d'une ligne dans `crates/ravum-core-machine/src/parser.rs`, plus le programme
de corpus `o7-ident-pattern-binds.rv`. Le test différentiel machine ↔ binaire
du CLI reste vert.

| Test | Résultat |
|---|---|
| `const` en expression (bornes `i64::MAX`, `i8::MIN`, `u8::MAX`), débordement via une constante, `K =>` (liaison) | 4 programmes identiques sur les 3 moteurs, 1 trap |
| motif de constante `crate::K` (la machine n'en a pas) | natif = interpréteur = valeurs calculées à la main |

## IR v1.2, suite — `Result`, `?` et fonctions génériques (2026-11-20, V1-P1)

- `Result<T, E>` : même disposition que les enums (`Ok` = 0, `Err` = 1) ;
  `Ok(..)`/`Err(..)`, motifs, `is_ok`/`is_err`/`unwrap_or`.
- `x?` sur `Option` et `Result` : retour anticipé de `None`/`Err(e)` (même
  type d'erreur exigé par le checker, donc aucune conversion), contrats
  `ensures` compris ; sinon la charge utile.
- **Fonctions génériques par monomorphisation** : une instance par jeu
  d'arguments de type (`pick<i64>`, `pick<f64>`), retrouvé en confrontant les
  types déclarés des paramètres et du retour aux types du checker à l'appel
  (`ravum-lang-ir::mono`). Instances créées à la demande, 1 024 au plus ;
  aucune fonction générique non instanciée n'est émise. Structs et enums
  génériques, `impl` générique : toujours `Unsupported`.

| Test (`ravum-lang-jit/tests/enums.rs`) | Résultat |
|---|---|
| `Result` + `?` en chaîne + `Option` + `?` (la machine n'a pas `?`) | 7 programmes : natif = interpréteur = valeurs calculées à la main |
| génériques : `pick<T>` à 2 instances, `swap<A, B>` sur tuple, `wrap<T> -> Option<T>`, récursion générique | natif = interpréteur = valeurs calculées à la main ; noms d'instances vérifiés |
| corpus machine | 55/61, inchangé |

## IR v1.3 — `String` et `Vec<T>` sur un tas d'exécution (2026-11-20, V1-P1)

**Conception** (`ravum-lang-ir/src/heap.rs`, partagé par l'interpréteur et le
JIT) :
- un objet (`String`, `Vec<T>`) est désigné par un descripteur de 8 octets,
  le type IR `Obj` ;
- un emprunt (`&String`, `&mut Vec<T>`, `&str`) est le descripteur lui-même ;
- copier un descripteur est un déplacement : le checker garantit qu'une
  valeur déplacée n'est plus utilisée ;
- **aucune libération pendant l'exécution** : le tas entier est libéré à la
  fin (`drop(x)` évalue seulement `x`) ;
- au-delà de 256 Mio alloués, l'exécution s'arrête sur `fault: heap limit`,
  jamais sur un plantage ;
- un module qui touche au tas est **refusé** pour une entrée temps réel
  (INV-05), comme un module qui imprime.

**Couvert** :
- `String::new`/`String::from`, `push_str`, `len`, `is_empty`, `clone`,
  `as_str`, `==`/`!=` de chaînes, littéraux, `x.to_string()` (scalaires),
  impression de toute chaîne ;
- `Vec::new`/`with_capacity`, `push`, `len`, `is_empty`, `clone`, `pop`
  (→ `Option`), `v[i]` en lecture et en écriture ;
- éléments scalaires ou descripteurs (`Vec<String>`) ;
- hors bornes : `index out of bounds: the len is N but the index is i`, le
  texte exact de `rustc` (EXPR-7.6).

**Toujours `Unsupported`** :
- `Vec` d'agrégats ;
- structs ou enums contenant une `String` (non `Copy`) ;
- les autres méthodes (`to_uppercase`, `trim`, `contains`, `lines`…) ;
- `for x in v`.

| Test | Résultat |
|---|---|
| `ravum-lang-cli/tests/jit_print_rustc.rs` : `String` et `Vec` passés par `&`/`&mut`, rendus par une fonction, `clone` indépendant, `Vec<f64>`, `Vec<String>`, `pop` jusqu'à `None`, accès hors bornes | **JIT = interpréteur = `rustc`**, sortie octet par octet (17 lignes) et même message de panique |
| **négatif** : entrée temps réel qui alloue | `Refusal::Invalid` (INV-05) |
| **négatif** : chaîne doublée 30 fois | `fault: heap limit` sur les deux moteurs |
| hot swap d'un `main` qui construit des chaînes | 1 fonction recompilée, nouvelle sortie |
| interpréteur | les instructions de tas sont dans une fonction non inlinée : sans cela, la frame de `call` grossissait et la récursion profonde du corpus débordait la pile native (trouvé par l'oracle) |

## Suite proposée

1. Élargir IR v1 : struct/tuple/tableau Copy, `match`, méthodes scalaires
   (`abs`, `checked_*`, `saturating_*`, `wrapping_*`), `math::*`, `i128`.
2. `ravum-lang run --backend jit` dans le CLI (touche un crate partagé).
3. T16 : le JIT rend le hot swap naturel (table d'indirection par fonction) ;
   à articuler avec `RFC-E11-HOT-RELOAD` déjà présent côté `rustc` sur `dev`.

## IR v1.6 (2026-11-25) — `Map<K, V>` (RFC-UI-LANG-1 P3)

- **Tas** : nouvel objet `Map`, avec un `BTreeMap` de clés typées (`Signed`,
  `Unsigned`, `Text`) vers des mots de valeur. L'ordre est celui de STD-20.2.
- **Opérations de tas** : `MapNew` (avec le code du type de clé), `MapLen`,
  `MapKeys`, `MapVals`, `MapSetVals`, `MapClone`.
- **Instructions à deux résultats** : `MapGet`, `MapInsert`, `MapRemove`
  (présence, puis valeur). Elles sont validées (indices et types) et
  partagées par l'interpréteur et le JIT (`rv_heap`, second résultat dans
  `ctx.scratch`).
- **Abaissement** :
  - les valeurs agrégées sont des vecteurs de mots, comme pour `Vec` ;
  - une vue `get_mut` d'un scalaire ou d'un agrégat est une copie, réécrite
    dans la table sur **toute** sortie : fin du `match` ou de l'instruction,
    `break`, `continue`, `return`, `?` ;
  - `for (k, v) in &mut m` parcourt un instantané, réécrit dans la table
    (`MapSetVals`) sur toute sortie de la boucle ;
  - le clone est profond.
- **INV-05** : le refus des instructions de tas dans un module temps réel
  passe désormais par `Inst::touches_heap_or_io`, à `match` exhaustif. La
  liste écrite à la main aurait laissé passer les nouvelles instructions
  `Map*`.
- **Oracle** : `run/map-ordered` et `run/map-word-count` donnent le même
  résultat avec rustc (`BTreeMap` de Rust, oracle indépendant),
  l'interpréteur et le JIT.

## IR v1.7 (2026-11-25) — primitives de texte (RFC-UI-LANG-1 P4)

- **Opérations de tas** : `StrFind` (indice, ou `u64::MAX` si absent),
  `StrStartsWith`, `StrEndsWith`, `StrSplit`, `StrTail`, `StrTruncate`,
  `StrChars`, `StrIsCharBoundary`, `StrJoin`. Une opération appelée hors de
  son domaine (séparateur vide, borne qui n'est pas une frontière de
  caractère) est une **faute** du tas, jamais un résultat partiel.
- **Abaissement** : les panics de la spec sont vérifiés **avant** l'opération
  de tas, par un `Trap` aux textes du prélude rustc (`empty separator`,
  `substring out of range`, `substring not on a char boundary`). `find` rend
  un `Option<usize>` construit d'après la sentinelle, jamais exposée au
  programme. `strip_prefix` et `strip_suffix` sont un test, puis une copie
  coupée (`StrTail` ou `StrClone` + `StrTruncate`). `starts_with` et
  `ends_with` sont aussi abaissés.
- **JIT** : `rv_heap` décode une opération par son indice dans `HeapOp::ALL`.
  Un test vérifie que cette liste est complète et dans l'ordre de
  déclaration. Avant, une liste écrite à la main dans le JIT devait suivre
  l'énumération. Les codes spéciaux de `rv_heap` passent de 20 à 64, pour
  ne plus pouvoir chevaucher les indices de `HeapOp`.
- **Oracle** : `run/text-primitives` donne la même sortie avec rustc,
  l'interpréteur et le JIT. Les cas limites et les cinq panics sont comparés
  entre l'interpréteur et le JIT (`crates/ravum-lang-jit/tests/text_primitives.rs`),
  et ont été vérifiés à la main avec rustc.

## IR v1.8 (2026-11-26) — `Arena<T>` / `Handle<T>` (RFC-UI-LANG-2 P2)

- **Tas** : objet `Arena` (places, générations, places libres triées) ; un
  compteur d'arènes par run (STD-21.5). Handle compacté en un mot `u64`
  (arène 20 bits, place 20 bits, génération 24 bits), donc ordonné comme
  (arène, place, génération) : clé de `Map` de type `Unsigned` (STD-21.11).
- **Opérations** : `ArenaNew`, `ArenaInsert` (nouvelles) ; `MapLen`,
  `MapKeys`, `MapVals`, `MapSetVals`, `MapClone` (nouvel identifiant),
  `MapGet`, `MapRemove`, `MapInsert` (réécriture d'une place vivante)
  servent aussi une arène. Les vues `get_mut` et les parcours `&mut` passent
  par le même mécanisme de réécriture que `Map`.
- **Arrêts** : `HeapStop::Panic` (`too many arenas in this run`, `arena is
  full`, STD-21.9), même texte dans l'interpréteur et le JIT
  (`CODE_HEAP_PANIC`).
- **Correctif** trouvé par le test adversarial : un `insert` qui réutilise
  une place, et `Map::insert` qui remplace une valeur, ne sont plus comptés
  comme des allocations. Avant, un long programme valide finissait en
  `heap limit` sur la voie C et pas sur la voie B.
- **INV-05** : une arène touche le tas ; un module temps réel qui en utilise
  une est refusé (`touches_heap_or_io`), donc aucun état migré ne contient
  d'arène (RFC-UI-LANG-2 §2.2).
- **Preuves** (`TESTED`) : `jit/tests/arena_parity.rs` (cas de conformance
  `arena-*` : sortie attendue par rustc = interpréteur = JIT),
  `jit/tests/arena_fuzz.rs` (fuzz différentiel B = interpréteur = JIT,
  20 programmes en mode rapide, 300 en mode long), `ir/tests/arena_heap.rs`
  (retrait d'une place à la génération `2^24 - 1` avec les vraies limites).
- Hors IR : les programmes à threads (comme `run/arena-handle-in-message`)
  restent refusés par l'IR, comme tout programme `std::thread`.
