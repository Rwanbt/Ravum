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




