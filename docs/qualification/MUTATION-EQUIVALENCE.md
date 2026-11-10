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
