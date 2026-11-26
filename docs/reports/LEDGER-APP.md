# Application d'essai `ledger` : couverture du langage et cas limites

Statut : `TESTED` sous Linux, toolchain `ravum-lang 0.1.0 (edition
core-1.0-draft.1)`, `rustc 1.95.0`, base `dev` @ `f07a1a9`. Code :
[`apps/ledger`](../../apps/ledger). Test d'intégration :
`ledger_app_works_end_to_end` dans
[`trial_apps.rs`](../../crates/ravum-lang-cli/tests/trial_apps.rs) (CI).

## Pourquoi cette application
Première application écrite **en Ravum** pour préparer U2 (PLAN UI) : un
petit gestionnaire de budget. Le domaine est simple, mais il oblige à
utiliser une grande part du langage et regorge de cas limites (montants
extrêmes, calendrier, texte Unicode, références périmées). Ce n'est pas
encore une application UI : elle sert d'abord à vérifier que le langage
porte une vraie application avant d'y brancher la fenêtre.

```text
ledger <journal>      0 : budgets respectés · 1 : budget dépassé · 2 : usage ou journal refusé
```

Journal : `budget <catégorie> <montant>` et
`AAAA-MM-JJ | catégorie/sous-catégorie | montant | libellé`, `#` commente.
Exemple : [`sample.journal`](../../apps/ledger/sample.journal), sortie
attendue : [`sample.expected`](../../apps/ledger/sample.expected).

## Structure
| Module | Rôle | Fonctionnalités exercées |
|---|---|---|
| `money.rv` | montants en centimes `i64`, lecture et affichage exacts | `const`, `chars()`, `checked_mul`/`checked_add`, `ok_or`, `?`, `i128`, `as` |
| `category.rv` | arbre des catégories | `Arena<T>`/`Handle<T>` (handles périmés, réutilisation de place), `Map<String, Handle>`, `impl` avec `&self`/`&mut self`, `get_mut`, `match` sur vue d'instruction, `for (k, v) in &arena` / `&map`, `split`, `trim` |
| `journal.rv` | lecture du journal, dates | `enum` à variantes record, `struct`, `Result<Option<T>, Error>`, `strip_prefix`, `find`, `substring`, `lines()` (CRLF), `match` sur entier, `match &x` (emprunts bornés) |
| `report.rv` | soldes, mois, budgets, plus grosse dépense | génériques sans bornes (`Ranked<T>`, `best<T>`), `for x in Vec` consommant, `Map<u32, i64>`, tuples, `std::thread` (`spawn`/`join`, contrôle croisé en `i128`), `panic` |
| `main.rv` | CLI | `mod` fichiers, `use std::*`, `env::args`, `fs::read_to_string`, `io::print`/`eprintln`, `process::exit` |

Tests : **21 `test fn`** (5 + 6 + 5 + 5), plus le test d'intégration :
sortie exacte sur l'exemple, codes de sortie, journal refusé sans aucune
sortie partielle, et les programmes refusés ci-dessous.

## Cas limites couverts
- **Montants** : `i64::MAX` centimes exact, un centime de plus refusé ;
  `i64::MIN` affiché sans débordement (`i128`) ; refus de `.5`, `1.`,
  `1.234`, `1,50`, `1e3`, `--1`, signe seul, vide, chiffres pleine chasse
  et arabes-indiens ; aller-retour lecture/affichage.
- **Totaux** : débordement d'un total rendu comme erreur (`total
  overflows`), jamais une panique ; somme parallèle en `i128` exacte là où
  `i64` déborde.
- **Calendrier** : années bissextiles (÷4, ÷100, ÷400), 31 avril, mois 0
  et 13, année 0, largeur de champ, séparateurs, chiffres non ASCII.
- **Texte** : libellé borné en **points de code** (80 « é » = 160 octets
  acceptés, 81 refusés) ; coupe jamais au milieu d'un caractère (emoji
  compris) ; `é` précomposé et `e` + accent combinant sont deux catégories
  distinctes (pas de normalisation) ; ordre des clés = octets UTF-8.
- **Handles** : feuille retirée → handle périmé refusé partout ; la place
  est réutilisée, l'ancien handle reste périmé ; retrait refusé si la
  catégorie a des enfants ou des mouvements ; remontée d'arbre bornée.
- **Journal** : CRLF, commentaires indentés, lignes vides, numéro de ligne
  dans chaque erreur, budget déclaré deux fois, mouvement nul, champs en
  trop ou manquants ; fail-closed (aucun mouvement partiel).

## Programmes refusés (`rejects/`, 20 cas)
Chaque fichier doit être refusé par `ravum-lang check` avec le code de
[`expected.txt`](../../apps/ledger/rejects/expected.txt) : vue de `Map`
stockée, ordre sur `Handle`, clé `f64`, closure, use-after-move, `Map`
modifiée pendant son parcours, motif `a | b`, trait, `==` sur `Option`,
`match` non exhaustif, `Arena` envoyée à un thread, effet non déclaré,
mélange `i64`/`i32`, référence retournée, move hors d'un `Vec`, `let`
de tuple, affectation d'un `let` immuable, identifiant Unicode, étiquette de
boucle, tranche `&v[0..1]`. Le test vérifie aussi que chaque fichier a son
code attendu.

## Constats sur le langage et l'outillage
1. **Code de diagnostic différent de la spec.** `let s: &[i32] = &a;`
   (`a: [i32; 2]`) est refusé avec `E-TYP-MISMATCH`, alors que
   [`types.md`](../../spec/language/types.md) Q-TYP-2 annonce
   `E-OWN-REF-ESCAPE`. Le refus est correct (fail-closed), seul le code
   diffère. À trancher : corriger le checker ou la spec. Non inclus dans
   `rejects/` pour ne pas figer l'un des deux.
2. **`fmt` déplace les commentaires de fin de ligne** sur la ligne
   suivante (`let x = 1; // un` devient deux lignes). C'est conforme à la
   spec (F-META-008 : idempotent, AST inchangé), mais le commentaire
   décrit alors la mauvaise ligne. L'application place ses commentaires
   au-dessus des lignes.
3. **Ergonomie, conforme à la spec mais coûteuse en pratique** :
   - littéraux `i32` par défaut, sans unification (F-TYP-020) :
     `unwrap_or(&0i64)`, `0usize..v.len()`, `get(&202610u32)` ;
   - `assert_eq` consomme ses arguments et exige le même type ; `==`
     n'accepte pas `&String` face à `&str` (liaison d'un `match` sur
     référence) : `x.as_str() == "…"` ;
   - pas de motif `a | b`, pas de `let _`, pas de `==` sur `Option` ;
   - un tableau n'a pas de méthode, et sa vue `&[T]` ne s'obtient qu'en
     argument.

   Aucun de ces points n'est un bug. Ce sont des candidats pour les RFC
   langage quand l'UI en aura besoin (U1).

## Non couvert par cette application
`async`/`.await`, `std::net`/`std::http`, FFI, profils `pure`/`rt`/`det`,
`watch --reload` (contrat `State`/`init`/`step`/`migrate`), JIT, canaux,
`mod` inline. Le rechargement à chaud (`watch --reload`) est la suite
naturelle pour U3 L2.
