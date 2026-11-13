# Équivalence des mutants survivants — `own.rs` (2026-11-10)

> **Statut : document de qualification, pas un certificat.** Il justifie, mutant
> par mutant, pourquoi dix mutants de `crates/ravum-lang-sema/src/own.rs`
> survivent à la campagne `cargo-mutants` alors que le checker est couvert. Un
> mutant **équivalent** ne change aucun comportement observable : aucun test ne
> peut le tuer, et en ajouter un qui prétendrait le faire serait un faux PASS.
>
> Les mutants **non** équivalents ont été tués par des tests réels (voir §3).

| Élément | Valeur |
|---|---|
| Outil | `cargo-mutants 27.1.0` |
| Package | `ravum-lang-sema@0.1.0` |
| Fichier | `crates/ravum-lang-sema/src/own.rs` |
| Portée de la passe | `eval_loop`, `eval_for`, `eval_call`, `eval_method_call` (22 mutants) |
| Résultat | **12 tués / 10 équivalents / 0 non expliqué** |
| Toolchain | `rustc 1.95.0`, Windows 11 x86-64 |
| Preuves | `target/mutants-loops/mutants.out/` (liste), `target/mutants-method/mutants.out/` (filtre receiver) |

## 1. Mutants équivalents — garde de point fixe des boucles (8)

Quatre mutants par boucle, aux lignes 1025–1028 (`eval_loop`) et 1105–1108
(`eval_for`) :

| Mutant | Ligne | Pourquoi il est équivalent |
|---|---|---|
| `iterations += 1` → `iterations *= 1` | 1025, 1105 | `iterations` reste à 0 : la sortie ne dépend plus que de `stable`. Le treillis de `State` est fini (un `SlotState` par slot, `partial` borné par les projections du corps) et `join` est monotone, donc `next == entry` est atteint en au plus quelques itérations pour tout programme représentable. La garde `iterations >= MAX_FIXPOINT` (16) est un filet défensif **inatteignable** ; la supprimer ne change pas la valeur finale de `entry`. |
| `next == entry` → `next != entry` | 1026, 1106 | Inverse la condition d'arrêt : une entrée stable ferait tourner la boucle jusqu'à `MAX_FIXPOINT` au lieu de s'arrêter. Or `entry = next` à chaque tour et `next` est identique une fois stable, donc `entry` final est le même. Une entrée non stable s'arrêterait au premier tour au lieu du seizième, mais aucune entrée non stable n'existe (cf. ligne précédente). |
| `stable \|\| iterations >= MAX` → `stable && iterations >= MAX` | 1028, 1108 | Ne s'arrête que si les deux conditions tiennent : une boucle stable tourne alors 16 fois au lieu de 2–3. `join` étant idempotent sur un état stable, `entry` final est inchangé ; seul le temps d'analyse change. |
| `iterations >= MAX` → `iterations < MAX` | 1028, 1108 | Pour une entrée stable, les deux formes s'arrêtent au premier tour stable. Pour une entrée non stable (inexistante), les deux s'arrêtent à `iterations == MAX`. Aucune différence observable. |

Ces quatre mutants sont donc **équivalents par construction**, pas par manque de
test. Le fait que `MAX_FIXPOINT` soit inatteignable est une propriété du modèle
(l'analyse est un point fixe sur un treillis fini), pas une hypothèse : si
`State` gagnait un composant non borné (par exemple un ensemble de loans
propagé), la garde deviendrait atteignable et ces mutants cesseraient d'être
équivalents. **Condition de réexamen : toute évolution de `State`, `join` ou
`SlotState` dans `own.rs`.**

## 2. Mutants équivalents — `eval_call` (2)

| Mutant | Ligne | Pourquoi il est équivalent |
|---|---|---|
| `delete !` dans `filter(\|p\| !p.is_receiver)` | 1171 | `CallTarget::UserFn` n'est produit que par `ValuePath::Item` (fonction libre) et `ValuePath::AssocFn`. `AssocFn` vient de `World::method(def, name, true)`, dont le prédicat est `is_associated()` = `receiver.is_none()` : ses paramètres n'ont donc **jamais** `is_receiver == true`. Le filtre ne retire rien. La version mutante garde les mêmes paramètres et produit la même carte argument→type. |
| `last_path_segment(&path) == "drop"` → `!=` | 1188 | Les deux branches sont équivalentes : `eval_call_arg(arg, None, true)` et `eval_value(arg)` ont le même effet observable quand `param` est `None` et `allow_ref_temporaries` est `true`. Les seules différences de `eval_call_arg` sont (a) la réborrow pour `Some(Type::Ref)` — impossible ici (`param == None`), (b) le rapport `E-OWN-REF-ESCAPE` quand `!allow_ref_temporaries` — faux ici, (c) le cas `is_static_ref`/`&expr` qui appelle `eval_value` dans les deux cas. Inverser le test déplace donc `drop(…)` et les autres builtins entre deux fonctions qui font la même chose. |

## 3. Mutant tué pendant cette passe — filtre receiver de `eval_method_call`

Le mutant `delete ! in OwnMethodCall` (ligne 1228) était le seul survivant
**réel** de la campagne précédente. Il est tué par
`crates/ravum-lang-sema/tests/own_audit.rs` :

- `audit_method_reference_argument_is_reborrowed_not_stored` — méthode
  `fn consomme(self, autre: &String)` appelée avec une **variable** référence
  `d.consomme(r)`. Sans le filtre, le checker prend `self: D` (non-référence)
  comme type du premier argument et émet une fausse fuite de référence.
- `audit_method_shared_receiver_accepts_reference_argument` — même ancrage pour
  un receiver `&self`.

Vérification : `cargo mutants -p ravum-lang-sema --file
crates/ravum-lang-sema/src/own.rs --regex eval_method_call` → **4 mutants,
4 tués** (dont la ligne 1228 ; le mutant `1254` était déjà tué).

## 4. Ce que ce document ne prouve pas

- Il ne prouve pas l'absence de mutants équivalents ailleurs : la passe est
  limitée à quatre fonctions de `own.rs`.
- L'argument d'équivalence est une **revue manuelle** du code et du treillis ;
  il n'est pas lui-même vérifié par un outil de preuve.
- Les mutants tués prouvent que les tests distinguent ces comportements ; ils ne
  prouvent pas que le checker est correct.

## 5. Campagne `typeck.rs` (passe 1, 2026-11-10)

Portée : quatre fonctions du vérificateur de types qui portent les chemins
d'erreur les plus denses — `check_lit_pattern`, `check_path_pattern`,
`cover_pattern` et `check_array`/`check_array_repeat`.

| Élément | Valeur |
|---|---|
| Commande | `cargo mutants -p ravum-lang-sema --file crates/ravum-lang-sema/src/typeck.rs --regex 'check_lit_pattern\|check_path_pattern\|cover_pattern\|check_array'` |
| Résultat | **49 mutants : 38 tués, 7 survivants expliqués, 4 unviables** |
| Preuves | `target/mutants-typeck2/mutants.out/` (49 mutants), `target/mutants-cover/mutants.out/` (12/12 tués sur `cover_pattern` après les ancres) |

Les sept survivants ne sont pas « à couvrir plus tard » en bloc : quatre sont
morts par construction, un est équivalent, un est hors de portée du harnais, et
un dépend d'une décision de spécification.

| Mutant | Ligne | Classement |
|---|---|---|
| `negative_ok` : `&&`→`\|\|`, `>`→`==`, `>`→`<`, `>`→`>=` | 4194 | **Mort par construction** : les motifs négatifs sont refusés par le parseur (`E-UNSUPPORTED-NEG-PAT`, voir `T2-FINDINGS.md` §16/D-PAT-1), donc la branche `*value > i128::MAX as u128` est inatteignable. Testable seulement si D-PAT-1 implémente G-6.37. |
| `delete match arm "Option::None"` | 4260 | **Équivalent** : le bras renvoie `expected` au lieu de `Type::Error` ; le type du motif n'est comparé à rien d'autre (l'exhaustivité utilise `pattern_kind`, pas `pat_types`), donc aucun diagnostic ni acceptation ne change. |
| `delete !` (compatibilité du motif de constante) | 4280 | **Hors portée du harnais** : un motif de constante n'est atteignable que par un chemin qualifié multi-module ; `cargo test` analyse un programme mono-fichier, et le corpus de conformance (qui a des modules) n'est pas lancé par `cargo-mutants`. L'équivalence n'est donc **pas** prouvée. |
| garde `*len == items.len()` → `true` | 3655 | **Non tranché** : le mutant ne change le diagnostic que pour une annotation de longueur contradictoire avec un premier élément divergent (`[i32; 3] = [panic(…), 1]` : `E-TYP-AMBIGUOUS` disparaît). La spécification ne dit pas si une annotation de longueur fausse fournit quand même le type d'élément ; classer ce mutant demanderait de trancher cette sémantique, pas d'écrire un test. |

La garde `→ false` et `==`→`!=` sont, elles, **tuées** par
`annotated_array_accepts_a_diverging_first_element` : sans la garde, un tableau
annoté de bonne longueur dont le premier élément diverge devenait
`E-TYP-AMBIGUOUS` au lieu d'être accepté (TYP-1.6).

Les autres mutants survivants de la campagne initiale ont été tués par sept
ancres ajoutées dans `crates/ravum-lang-sema/tests/typeck_contracts.rs` :
plage de motif entier, motifs littéraux des trois autres familles sur une
scrutinee `!`, tableau répété annoté, et couverture wildcard pour `bool`, enum,
`Option` et `Result` (12/12 mutants de `cover_pattern` tués après cette ancre).

## 7. Campagne étendue — `own.rs`, `resolve.rs`, `typeck.rs` (2026-11-10)

Commande (Windows 11 x86-64, rustc 1.95.0, `cargo-mutants 27.1.0`, `--no-shuffle -j 8`) :

```
cargo mutants --file crates/ravum-lang-sema/src/own.rs    -F "borrow_place|check_availability|place_mutability"
cargo mutants --file crates/ravum-lang-sema/src/resolve.rs -F "resolve_type_path|resolve_imports|resolve_use_target|type_is_copy|nominal_is_copy"
cargo mutants --file crates/ravum-lang-sema/src/typeck.rs  -F "check_struct_lit|check_variant_call|check_for|check_lit_pattern|cover_pattern|negative_value_fits|pattern_contains_float|check_const_expr"
```

| Fichier | Mutants | Tués | Survivants | Invivables | Durée |
|---|---|---|---|---|---|
| `own.rs` | 32 | 31 | 1 | 0 | 2 min |
| `resolve.rs` | 74 | 51 | 21 | 2 | 4 min |
| `typeck.rs` | 106 | 71 | 28 | 7 | 5 min |
| **Total** | **212** | **153** | **50** | **9** | **11 min** |

(`own.rs` : première campagne 30 tués / 2 survivants ; le test
`audit_partial_move_then_borrow_is_use_after_move_not_double_move` ajouté
ensuite tue le mutant L1455, revérifié par une seconde campagne ciblée
`-F "check_availability"` : **1 survivant, 28 tués**.)

Les 50 survivants sont triés en quatre catégories. Un survivant n'est **pas**
« équivalent » par défaut : chaque ligne ci-dessous donne la raison, et la
catégorie « lacune de test » est un aveu, pas une excuse.

### 7.1 Morts par construction (inatteignables) — 14 mutants

Preuve de non-atteignabilité : `parser.rs::parse_path` transforme les mots-clés
`crate` / `self` / `super` en `PathRoot::{Crate, SelfMod, Super}` **avant** toute
segmentation, et `Self` est `Kw::SelfType` (il ne peut pas être un segment
`Ident`). Les branches `PathRoot::Plain` qui comparent le premier segment à
`"crate"`, `"self"`, `"super"` ou `"Self"` ne s'exécutent donc jamais.

| Mutants | Emplacement |
|---|---|
| delete match arm "crate"/"self"/"super", `==`→`!=`, `+=`→`*=`/`-=` | `resolve.rs` `resolve_use_target` L1290-1301 (6) |
| idem | `resolve.rs` `resolve_type_path` L2388-2399 (6) |
| `==`→`!=`, `!=`→`==` sur `segments[0] == "Self"` | `resolve.rs` `resolve_type_path` L2268-2269 (2) |

### 7.2 Observables seulement par le codegen (faits de résolution) — 4 mutants

Le harnais de mutation exécute `cargo test -p ravum-lang-sema`, qui n'émet pas de
Rust et ne consomme pas les faits (`facts.resolutions`) : ces mutants ne peuvent
pas être tués par un test de la crate sema.

| Mutant | Emplacement | Ce qui le tuerait |
|---|---|---|
| delete match arm `ValuePath::Type`, `::Variant`, `::BuiltinConst` | `typeck.rs` `check_struct_lit` L3840/3845/3854 | un littéral de struct/variante compilé par le corpus (`cargo run -p ravum-conformance -- run`) |
| delete match arm `Type::F32` | `typeck.rs` `check_lit_pattern` L4226 | un motif flottant sur un scrutation `f32` : le suffixe émis devient `f64` et `rustc` refuse. Le corpus `parser/match-float-literal-pass` contient `fn simple(x: f32)` et couvre ce chemin par la compilation, pas par le harnais sema |

### 7.3 Équivalent par absence d'observateur — 1 mutant

| Mutant | Emplacement | Raison |
|---|---|---|
| `!st.partial.is_empty() && !whole_reinit` → `\|\|` | `own.rs` `check_availability` L1417 | Le bloc ne s'exécute alors que `st.partial` est vide **et** `whole_reinit` vrai : la boucle ne fait rien, `relation` reste `Disjoint`, et `allowed` est vrai pour `Access::Write` comme pour les autres accès. Aucun diagnostic, aucun changement d'état. |

### 7.4 Lacunes de test — 31 mutants (défaut de preuve, non corrigé)

Ces mutants **changent** un comportement observable, mais aucun test actuel ne
distingue la version mutée. Ils sont la feuille de route des prochaines passes.

Le mutant `own.rs` `check_availability` L1455 (`&&`→`\|\|`) n'est **plus** dans
cette catégorie : `audit_partial_move_then_borrow_is_use_after_move_not_double_move`
le tue (déplacement partiel puis emprunt nu ⇒ `E-OWN-USE-AFTER-MOVE`, pas
`E-OWN-DOUBLE-MOVE`), revérifié par une campagne ciblée.

| Cluster | Mutants | Ce qu'un test devrait observer |
|---|---|---|
| `typeck.rs` `check_struct_lit` L3890/3896/3953 (`==`/`&&`, longueur d'arguments) | 8 | inférence des arguments génériques d'un littéral de struct/variante depuis le type attendu : `fn f() -> Enveloppe<i32> { Enveloppe { v: 1 } }` puis usage typé de `v` |
| `typeck.rs` `check_const_expr` L4733-4864 (`<<`, gardes de type, `not` booléen/entier, bras `Block`) | 11 | évaluer la **valeur** d'une constante : longueur de tableau (`const N: usize = 1 << 2; let a: [i32; N]`), `!true`, `!0`, constante de type `f64` (doit rester `E-RES-CONST-NOT-CONST`), bloc constant |
| `typeck.rs` `check_variant_call` L2243 | 1 | appel d'une variante avec un argument de type incompatible |
| `typeck.rs` `check_for` L3381 (`-=`→`+=`, `/=` ) | 2 | compteur de boucle décrémenté dans un `for` sur plage |
| `resolve.rs` `resolve_use_target` L1315/1347 (`Sym::BuiltinModule`) | 2 | `use math;` / import d'un sous-module builtin (chemin `Ns::Module` → `BuiltinModule`) |
| `resolve.rs` `resolve_type_path` L2348/2427/2476 (`\|\|`→`&&`, bras `BuiltinModule`) | 3 | un module utilisé comme type (`fn f(x: math)`) et un type utilisé comme module (`Option::X` en type) |
| `resolve.rs` `type_is_copy` L2792/2795 (`&&`/`\|\|`) | 2 | décider si un `ref struct`/`ref` de `String` est `Copy` (déplacement vs copie observé par un diagnostic d'ownership) |

Aucun chiffre de score de mutation n'est publié : 153/212 mutants tués sur un
périmètre **ciblé** ne dit rien du reste de la crate (voir §1 pour la règle).

## 8. Campagne après la passe 5 — 2026-11-10

Les correctifs de `T2-FINDINGS.md` §20 ont changé le code des trois fichiers, et
`crates/ravum-lang-sema/tests/mutation_gaps.rs` (26 contrats, plus une
assertion ajoutée en clôture) a été ajouté pour tuer les survivants qui étaient
de vraies lacunes. Trois campagnes ont été exécutées :

1. **avant** les nouveaux contrats (`target/mut5/`) : 195 mutants, 138 tués,
   47 non tués, 9 invivables, 1 timeout ;
2. **après** (`target/mut6/` et `target/mut7/`), mêmes filtres, même machine
   (Windows 11 x86-64, rustc 1.95.0, `cargo-mutants 27.1.0`, `--no-shuffle -j 8`) :

| Fichier | Mutants | Tués | Survivants | Invivables | Timeout | Durée |
|---|---|---|---|---|---|---|
| `own.rs` | 32 | 31 | 1 | 0 | 0 | 2 min |
| `resolve.rs` | 57 | 40 | 15 | 2 | 0 | 4 min |
| `typeck.rs` | 106 | **95** | 4 | 7 | 0 | 6 min |
| **Total** | **195** | **166** | **20** | **9** | **0** | **13 min** |

Le total de mutants est passé de 212 à 195 parce que `resolve.rs` a été corrigé
(bras de code supprimés, chemins simplifiés). Le taux de tués sur ce périmètre
ciblé passe de **153/212 (72,2 %)** à **166/195 (85,1 %)** — ce chiffre ne vaut
que pour ces trois ensembles de fonctions (voir §1).

Le `timeout` observé en campagne 1 (`typeck.rs::check_for` L3475, `-=` → `+=`)
a disparu : le mutant est désormais tué, et le `-=`/`+=` de `check_for` est
couvert par `continue_after_a_for_loop_is_outside_the_loop`. L'assertion
supplémentaire de `struct_variant_literal_records_a_variant_resolution`
(`Resolution::Error` interdit) tue le dernier mutant observable de
`check_struct_lit` (L3939, preuve `target/mut7/typeck-structlit.out/`) : la
fonction est **14/14 sur les mutants observables** (18 mutants : 14 tués,
3 équivalents, 1 invivable).

### 8.1 Survivants restants (20) — tri

**Morts par construction, 14 mutants.** `parser::parse_path` transforme les
mots-clés `crate`/`self`/`super` en `PathRoot::{Crate, SelfMod, Super}` **avant**
toute segmentation, et `Self` est `Kw::SelfType` (jamais un segment `Ident`) :
les bras `PathRoot::Plain` qui comparent le premier segment à ces chaînes ne
s'exécutent jamais. Même preuve que §7.1, revérifiée après les correctifs.

| Mutants | Emplacement |
|---|---|
| delete `crate`, `self`, `super` ; `==`→`!=` ; `+=`→`-=`/`*=` | `resolve.rs` `resolve_use_target` L1321-1326 (6) |
| delete `crate`, `self`, `super` ; `==`→`!=` ; `+=`→`-=`/`*=` ; `==`/`!=` sur `Self` | `resolve.rs` `resolve_type_path` L2422-2553 (8) |

**Équivalents — 6 mutants au total.** Deux d'abord :

| Mutant | Emplacement | Raison |
|---|---|---|
| `&&` → `\|\|` | `own.rs` `check_availability` L1423 | même raisonnement que §7.3 : le bloc ne s'exécute que si `partial` est vide et `whole_reinit` vrai ; `relation` reste `Disjoint` et `allowed` est vrai pour tout accès. |
| `\|\|` → `&&` | `resolve.rs` `type_is_copy` L2949 | le test de gauche est `*def == PRELUDE_ERROR_DEF` ; le test de droite (`nominal_kind(def).is_none()`) est **toujours faux** pour `Error` (type prélude sans item, donc sans genre nominal). `true && false` = `false` ⇒ les deux formes renvoient `false` (mutant de gauche) ou passent à `nominal_is_copy(Error)`, qui rend `false` (pas de `struct` utilisateur). Résultat identique dans les deux cas ; `result_with_a_mobile_error_is_not_copy` exerce le chemin. |

Quatre autres, dont l'équivalence est prouvée par le code, pas supposée :

| Mutant | Emplacement | Raison |
|---|---|---|
| delete `!`, `&&`→`||`, `==`→`!=` (3) | `typeck.rs` `check_struct_lit` L3990 | `args` vient de `ValuePath::Type(Type::Nominal { args, .. })` et **tous** les `ValuePath::Type` sont construits avec `args: Vec::new()` (`type_of_symbol`, `prelude_type`, `error_type`) : la garde est morte, sa boucle est un no-op, et les trois formes gardent le même comportement. Réexamen si un chemin de valeur porte un jour des arguments génériques. |
| `&&`→`||` | `typeck.rs` `check_const_expr` L4820 | `ty` sort toujours de `check_int_literal` (suffixe, type attendu entier, sinon `i32`), donc `ty.is_int()` est toujours vrai ; les deux formes ne diffèrent que si `int_value_fits` est faux, cas où `check_int_literal` a **déjà** émis `E-TYP-LITERAL-RANGE` avec le même prédicat et où `check()` rend `failed` (`facts` jeté). Aucun programme accepté ni diagnostic ne change. |

**Lacunes de test résiduelles : 0.** Les cinq mutants classés « lacunes » dans
la campagne 2 sont soit tués (L3939), soit ramenés à des équivalents prouvés
(L3990 ×3, L4820).

### 8.2 Ce que cette campagne ne prouve pas

- Rien sur les 3 fichiers hors de ces ensembles de fonctions, ni sur les autres
  crates : 166/195 est un chiffre **ciblé** (§1).
- Les mutants « morts par construction » sont prouvés par l'analyse du parseur,
  pas par un test qui échouerait ; c'est une preuve statique, pas dynamique.
- La CI rejoue ces trois campagnes chaque semaine (`.github/workflows/mutation.yml`,
  job d'évidence non bloquant : les survivants sont triés ici, pas masqués).

## 9. Passe 7 (2026-11-11, `1376449`) — tri des 24 survivants

Campagne relancée **localement** (le workflow `mutation.yml` est en
`schedule`/`dispatch` et n'existe pas sur la branche par défaut). Arbre propre,
HEAD `1376449`, `cargo-mutants 27.1.0`, `rustc 1.95.0`, Windows 11 x86-64,
`--no-shuffle -j 4`, mêmes `--file`/`-F` que `mutation.yml` :

```
cargo mutants --no-shuffle -j 4 --file crates/ravum-lang-sema/src/own.rs \
  -F 'borrow_place|check_availability|place_mutability'
cargo mutants --no-shuffle -j 4 --file crates/ravum-lang-sema/src/resolve.rs \
  -F 'resolve_type_path|resolve_imports|resolve_use_target|type_is_copy|nominal_is_copy'
cargo mutants --no-shuffle -j 4 --file crates/ravum-lang-sema/src/typeck.rs \
  -F 'check_struct_lit|check_variant_call|check_for|check_lit_pattern|cover_pattern|negative_value_fits|pattern_contains_float|check_const_expr'
```

| Fichier | Mutants | Tués | Survivants | Non viables | Durée |
|---|---|---|---|---|---|
| `own.rs` | 32 | 31 | 1 | 0 | 3 min |
| `resolve.rs` | 57 | 40 | 15 | 2 | 5 min |
| `typeck.rs` | 94 | 79 | 8 | 7 | 7 min |
| **Total** | **183** | **150** | **24** | **9** | **15 min** |

`typeck.rs` compte 94 mutants au lieu des 106 de §8 : la réécriture de
l'exhaustivité (matrice de constructeurs récursive) a supprimé `Coverage` et
`cover_pattern`, donc douze sites de mutation. Preuves : `target/mutants-p7/`
(`missed.txt`, `diff/`, `caught.txt`, `unviable.txt` pour chaque campagne).

### 9.1 Tri — 24/24 équivalents, 0 lacune de test

Aucun test n'est ajouté : un test qui ne peut pas échouer sur le code muté
serait un faux PASS (règle de §1 et de `tests/mutation_gaps.rs`). Chaque ligne
renvoie à la preuve détaillée de sa classe, en §9.2.

| # | Mutant (`fichier:ligne:col`) | Classe | Argument court |
|---|---|---|---|
| 1 | `own.rs:1525:35` `&&`→`\|\|` dans `check_availability` | E1 | garde de balayage sans effet sur le verdict (voir E1) |
| 2 | `resolve.rs:1321:17` delete `"crate"` (`resolve_use_target`) | E2 | bras `PathRoot::Plain` inatteignable (parseur) |
| 3 | `resolve.rs:1322:17` delete `"self"` | E2 | idem |
| 4 | `resolve.rs:1323:17` delete `"super"` | E2 | idem |
| 5 | `resolve.rs:1325:78` `==`→`!=` (boucle `super`) | E2 | idem (et `super::super` est une erreur de parse) |
| 6 | `resolve.rs:1326:32` `+=`→`-=` | E2 | idem |
| 7 | `resolve.rs:1326:32` `+=`→`*=` | E2 | idem |
| 8 | `resolve.rs:2426:18` `==`→`!=` (bras `Self`) | E2 | `Self` n'est jamais un chemin (`Kw::SelfType`) |
| 9 | `resolve.rs:2427:27` `!=`→`==` (bras `Self`) | E2 | idem |
| 10 | `resolve.rs:2546:17` delete `"crate"` (`resolve_type_path`) | E2 | bras `PathRoot::Plain` inatteignable |
| 11 | `resolve.rs:2550:17` delete `"self"` | E2 | idem |
| 12 | `resolve.rs:2554:17` delete `"super"` | E2 | idem |
| 13 | `resolve.rs:2556:78` `==`→`!=` (boucle `super`) | E2 | idem |
| 14 | `resolve.rs:2557:32` `+=`→`-=` | E2 | idem |
| 15 | `resolve.rs:2557:32` `+=`→`*=` | E2 | idem |
| 16 | `resolve.rs:2953:52` `\|\|`→`&&` dans `type_is_copy` | E3 | `Error` est le seul nominal sans genre : les deux opérandes y sont vrais |
| 17 | `typeck.rs:4027:20` delete `!` (`check_struct_lit`) | E4 | `args` est toujours vide (arguments génériques refusés en expression) |
| 18 | `typeck.rs:4027:37` `&&`→`\|\|` | E4 | idem : la boucle sur `args` est un no-op |
| 19 | `typeck.rs:4027:51` `==`→`!=` | E4 | idem |
| 20 | `typeck.rs:4857:44` `&&`→`\|\|` (`check_const_expr`) | E5 | la valeur mutante n'existe que dans un `check` déjà en échec (`E-TYP-LITERAL-RANGE`) |
| 21 | `typeck.rs:6009:5` `pattern_contains_float` → `false` | E6 | filtre redondant depuis la matrice de couverture récursive |
| 22 | `typeck.rs:6010:9` delete bras `PatKind::Lit(Float)` | E6 | idem |
| 23 | `typeck.rs:6011:9` delete bras `PatKind::Tuple` | E6 | idem |
| 24 | `typeck.rs:6012:9` delete bras `PatKind::Variant` | E6 | idem |

### 9.2 Preuves par classe

**E1 — `own.rs` `check_availability` L1525 (`&&`→`||`).** Le bloc ne s'exécute
que si `!st.partial.is_empty() && !whole_reinit`. Mutant : il s'exécute aussi
quand l'un des deux seulement est vrai. (a) `partial` vide, pas de
réinitialisation totale : la boucle ne parcourt rien, `relation` reste
`Disjoint`, et `allowed` est vrai pour tous les accès (branche `Write` :
`Disjoint` est admis ; autres accès : `relation == Disjoint`). (b)
réinitialisation totale (`access == Write && steps.is_empty()`) avec des
déplacements partiels : `classify_partial(&[], moved)` rend toujours `Ancestor`
(préfixe vide), et la branche `Write` admet `Ancestor` ; aucun diagnostic n'est
émis, aucun état n'est modifié. Le bloc n'a donc aucun effet observable dans les
deux cas ; même conclusion que §7.3 et §8.1, revérifiée sur le code courant
(`classify_partial`, `allowed`).

**E2 — `resolve.rs`, bras `PathRoot::Plain` et bras `Self` (14 mutants).**
`parser::parse_path` consomme `crate`/`self`/`super` comme `PathRoot` **avant**
toute segmentation (`parser.rs:2204-2222`) ; `Self` est `Kw::SelfType` et devient
`TypeKind::SelfType` (`parser.rs:2051-2069`), résolu directement
(`resolve.rs:2259`), jamais un segment `Ident`. Les bras `Plain` comparant le
premier segment à ces chaînes, et le bras `Self` de `resolve_type_path`, ne
s'exécutent donc jamais. Mesures : `use crate::…`, `use self::…`,
`use super::…`, `crate::…`/`super::…` en type et `Self` en annotation sont
acceptés par les bras `PathRoot::*` ; `use super::super::S` est refusé par le
parseur (`E-PARSE-EXPECTED`, « expected path segment, found keyword `super` »),
donc la boucle `levels` est morte elle aussi. Même preuve que §7.1/§8.1.

**E3 — `resolve.rs` `type_is_copy` L2953 (`||`→`&&`).** `PRELUDE_ERROR_DEF`
vaut `NodeId::MAX` (`builtins.rs:31`) et n'a ni `struct` ni `enum` dans le
`World` : pour `Error`, `def == PRELUDE_ERROR_DEF` **et**
`nominal_kind(def).is_none()` sont tous deux vrais, donc `||` et `&&` renvoient
`false` de la même façon. Pour tout autre nominal atteignable, les deux
opérandes sont faux. La branche mutante n'est donc jamais prise seule ; même si
elle l'était, `nominal_is_copy(Error)` rend `false` (champ `String`). Le chemin
est exercé par `result_with_a_mobile_error_is_not_copy`.

**E4 — `typeck.rs` `check_struct_lit` L4027 (3 mutants).** `args` provient de
`ValuePath::Type(Type::Nominal { args, .. })`, c'est-à-dire des arguments
génériques explicites d'un littéral de struct. Le parseur les refuse en
expression : `Boite::<i32> { contenu: 1 }` rend
`E-PARSE-UNEXPECTED` (« explicit generic arguments are not supported in
expressions », mesuré). `args` est donc toujours vide : `delete !` et
`&&`→`||` ne changent qu'une condition dont le corps est une boucle sur un
vecteur vide, et `==`→`!=` ne fait que sauter cette boucle vide. Preuve
indépendante (déjà §8.1) : tous les `ValuePath::Type` sont construits avec
`args: Vec::new()`.

**E5 — `typeck.rs` `check_const_expr` L4857 (`&&`→`||`).** `check_int_literal`
émet `E-TYP-LITERAL-RANGE` dès que `int_value_fits` est faux
(`typeck.rs:1385-1391`), c'est-à-dire exactement le cas où les deux formes
diffèrent. La valeur `ConstValue::Int` supplémentaire n'existe donc que dans un
`check` déjà en échec (`checked == None`, faits jetés, codegen non lancé) :
aucun programme accepté ni diagnostic ne change. Mesure : `const X: u8 = 300;`
→ `E-TYP-LITERAL-RANGE` dans les deux formes.

**E6 — `typeck.rs` `pattern_contains_float` (4 mutants).** Depuis la matrice de
couverture récursive, `patterns_exhaustive` décide par constructeur ; un
sous-motif flottant ne peut être typé que contre `f32`/`f64`, et
`coverage_ctors` ne fournit **aucun** constructeur fini pour ces types. Une
ligne de motifs contenant un flottant ne peut donc jamais être la ligne
irréfutable qui couvre cette colonne : la retirer (filtre) ou la garder (mutant)
ne change aucun verdict d'exhaustivité. Le prédicat reste un garde de
cohérence codegen (G-6.36 : un bras à garde ne couvre rien). Preuve dynamique :
copie mutée de la crate (`pattern_contains_float → false`) construite sous
`target/probes/equiv-float/` et comparée au driver normal sur 18 programmes
(f64/f32 de premier niveau, `Option<f64>`, `Option<Option<f64>>`, tuple
`(f64, bool)`, variantes d'enum tuple et struct, variantes avec `_`) :
**18/18 verdicts identiques**. Les tests existants
`float_literal_pattern_requires_a_wildcard`,
`coverage_engine_tracks_pattern_shapes` et
`float_literal_pattern_on_int_is_mismatch` pinent la règle ; le mutant
`pattern_contains_float → true` est, lui, **tué** (il rend tout `match`
non exhaustif). Rappel : G-6.36 (MUST) **accepte** un motif flottant ; il exige
seulement qu'un `_`/liaison couvre la colonne, faute de quoi
`E-TYP-NONEXHAUSTIVE`.

### 9.3 Conditions de réexamen

Les équivalences E1–E6 dépendent de propriétés qui peuvent changer :

- E1 : toute évolution de `State`, `partial` ou `classify_partial` dans `own.rs` ;
- E2 : un parseur qui accepterait `crate`/`self`/`super`/`Self` comme segments,
  ou `super::super::…` comme chemin (les bras deviendraient atteignables) ;
- E3 : un `Type::Nominal` atteignable sans genre nominal autre que `Error` ;
- E4 : des arguments génériques explicites acceptés en expression ;
- E5 : une évaluation de constante qui enregistre une valeur avant/pendant le
  contrôle de plage ;
- E6 : un changement de `patterns_exhaustive`/`coverage_ctors` tel qu'une
  colonne `f32`/`f64` puisse être considérée couverte.

### 9.4 Ce que cette passe ne prouve pas

- Les arguments E1–E6 sont des **revues statiques**, pas des preuves vérifiées
  par un outil ; la batterie E6 est une évidence sur 18 programmes, pas une
  preuve sur toutes les entrées.
- Rien sur les fichiers hors des trois ensembles de fonctions ciblés, ni sur les
  autres crates : 150/183 est un chiffre **ciblé** (§1), pas un score de crate.
- La campagne n'a pas été exécutée par la CI sur ce commit (workflow non
  déclenchable sur la branche par défaut) : les artefacts sont locaux
  (`target/mutants-p7/`, non versionnés).





## 10. Première campagne CI (2026-11-13, run `43326219422`)

Le workflow `mutation.yml` est désormais sur `main` ; il a été déclenché une
première fois (`workflow_dispatch`, 2026-11-12 23:08 UTC) sur `e6ca298`
(code `2b31d63`), `ubuntu-latest`, `rustc 1.95.0`, `cargo-mutants 27.1.0`,
`--no-shuffle -j 8`, mêmes `--file`/`-F` que §9.

| Fichier | Mutants | Tués | Survivants | Non viables |
|---|---|---|---|---|
| `own.rs` | 32 | 31 | 1 | 0 |
| `resolve.rs` | 57 | 40 | 15 | 2 |
| `typeck.rs` | 94 | 79 | 8 | 7 |
| **Total** | **183** | **150** | **24** | **9** |

**Reproductibilité observée** : la campagne CI retrouve **exactement** les
totaux de la campagne locale de §9 (`1376449`, 2026-11-11), alors que les passes
8 à 10 ont déplacé les lignes et ajouté du code dans `typeck.rs`/`own.rs`. Les
24 survivants appartiennent donc aux mêmes familles E1–E6 déjà triées en §9.1 ;
aucun nouveau survivant n'est apparu, et aucun tri supplémentaire n'est requis.

**Défaut trouvé par ce premier run** : l'étape « Summarize outcomes » lisait
`outcomes.json` comme une **liste**, alors que `cargo-mutants` 27.x écrit un
**objet** `{"outcomes": […], "total_mutants": N, "missed": M, …}`. Le script
levait `AttributeError: 'str' object has no attribute 'get'` et le run
apparaissait rouge **alors que les trois campagnes avaient réussi**. Corrigé
dans la branche `fix/mutation-summary` : lecture du bon schéma, impression
nominative de chaque survivant (`MISSED <mutant>`), comportement non bloquant
conservé (les survivants sont triés, pas fatals).

**Ce que cette campagne ne prouve pas** : 150/183 est un chiffre **ciblé** sur
trois ensembles de fonctions, pas un score de crate ; le tri E1–E6 reste une
revue statique ; la campagne n'ajoute aucun test. Preuves : artefact
`mutants-targeted` (rétention 90 jours) du run
[43326219422](https://github.com/Rwanbt/Ravum/actions/runs/43326219422).
