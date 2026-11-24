# RFC-UI-LANG-1 — Prérequis du langage pour Ravum UI

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-UI-LANG-1` |
| Statut | **PROPOSED**, à décider par le responsable |
| Date | 2026-11-24 |
| Auteur | agent de développement |
| Cadre | [`DECISIONS-2026-11-24-UI.md`](../decision/DECISIONS-2026-11-24-UI.md) (UI-D1, UI-D2, UI-D5), étape U0 de [`docs/ui/PLAN.md`](../ui/PLAN.md) |
| Portée | `spec/language/` (expressions, ownership, stdlib), `ravum-lang-sema`, `ravum-lang-codegen`, `ravum-lang-ir`/`ravum-lang-jit`, conformance |

## 1. Contexte

Ravum UI suit un modèle à messages (UI-D2) :
- `view(&Model) -> Node<Msg>` construit un arbre ;
- `update(&mut Model, Msg)` modifie l'état ;
- le runtime parcourt l'arbre pour la mise en page, le rendu et le routage des événements.

Ce que Core 1.0 accepte déjà, vérifié le 2026-11-24 :
- les enums génériques récursifs (`enum Node<M> { Leaf(M), Many(Vec<Node<M>>) }`) ;
- `match` **par valeur** sur ces enums, la construction de l'arbre et `&mut Model`.

Ce qui manque, constaté sur des programmes réels :
- les applications d'essai [`V2-TRIAL-APPS.md`](../reports/V2-TRIAL-APPS.md), limites L1 à L3 ;
- un prototype d'arbre de nœuds.

Chaque manque ci-dessous empêche d'écrire une application UI sans contorsion.

## 2. Principe commun

Aucune proposition ne change le modèle d'ownership de Core 1.0 : **une
référence ne vit pas au-delà de la construction qui la crée** (OWN-6). Les
nouvelles références sont toutes **limitées à un bras de `match` ou à un
corps de boucle**, et ne sont ni stockables ni retournables. Aucune durée de
vie explicite n'est introduite.

## 3. Propositions

### P1 — `match` sur une référence (bloquant)

Aujourd'hui, EXPR-5.10 fait consommer son scrutinee à tout `match`, et
`match n` avec `n: &Node<M>` est refusé (`E-TYP-MISMATCH`). Le runtime ne
peut donc pas lire un arbre sans le détruire.

Proposition : sur un scrutinee de type `&E` (ou `&mut E`), les liaisons d'un
motif sont des **références** (`&T`, ou `&mut T`) au champ correspondant,
valables dans le bras seulement. Le scrutinee n'est pas consommé. Un champ
`Copy` peut être lu par `*x`.

```rv
fn count(n: &Node<Msg>) -> i64 {
    match n {
        Node::Leaf(_) => 1,
        Node::Many(kids) => kids.len() as i64,   // kids: &Vec<Node<Msg>>
    }
}
```

Tests négatifs :
- stocker ou retourner `kids` donne `E-OWN-REF-ESCAPE` ;
- déplacer `*kids` donne `E-OWN-MOVE-OUT-OF-BORROW` ;
- avec `&mut`, un emprunt concurrent du scrutinee donne `E-OWN-BORROW-CONFLICT`.

### P2 — `for x in &v` et `for x in &mut v`

Aujourd'hui, EXPR-6.4 n'autorise `for` que sur un intervalle ou un `Vec`
possédé ; une liste empruntée se parcourt par index.

Proposition : `for x in &v` lie `x: &T`, et `for x in &mut v` lie `x: &mut T`,
pour le corps seulement. `v` reste emprunté pendant la boucle : `v.push(..)`
dans le corps donne `E-OWN-BORROW-CONFLICT`.

### P3 — Table associative `Map<K, V>`

Aujourd'hui, aucune table n'existe (STD-1.3). Le runtime en a besoin pour les
états par identifiant de widget, les registres de services et les styles
nommés.

Proposition : un type de prélude `Map<K, V>`, avec `K` limité aux entiers,
`bool`, `char` et `String`.
- Méthodes : `new`, `len`, `is_empty`, `insert(k, v) -> Option<V>`,
  `get(&k) -> Option<&V>` (référence limitée à l'instruction),
  `remove(&k) -> Option<V>`, `contains_key(&k)`.
- Parcours : `for (k, v) in &m`, dans l'**ordre croissant des clés**. Cet ordre
  est déterministe, alors qu'un ordre de hachage dépendrait de la plateforme
  et casserait la vérification déterministe.
- Implémentation de référence : un arbre équilibré (`BTreeMap` côté `rustc`).

### P4 — Opérations de texte

Aujourd'hui, STD-9.11 exclut `split`, `find` et les sous-chaînes. Un champ de
saisie et un rendu de texte en ont besoin.

Proposition, sur `String` et `&str`, avec des positions en **octets** UTF-8
comme `len()` (STD-9.2) :
- `find(s) -> Option<usize>` ;
- `split(sep) -> Vec<String>` (un `sep` vide panique) ;
- `strip_prefix(s) -> Option<String>` et `strip_suffix(s) -> Option<String>` ;
- `substring(start, end) -> String`, qui panique si une borne n'est pas une
  frontière de caractère ou dépasse `len()` ;
- `chars() -> Vec<char>`.

### P5 — `clone` structurel

Aujourd'hui, `clone()` n'existe que pour `Vec<T>` avec `T: Copy` (STD-10.4).
Pourtant les messages portés par l'arbre (`Msg::Rename(String)`) doivent être
copiés quand un événement est routé.

Proposition : `clone()` est défini sur toute struct, tout enum, tout `Vec`,
tout `Option` et tout `Result` dont tous les champs sont clonables. La règle
est structurelle, sans trait, et vérifiée par le checker ; `String` est
clonable. Un type contenant une ressource de `std` qui ne se clone pas (s'il
en existe) donne `E-RES-UNKNOWN`.

## 4. Hors périmètre de cette RFC

- **Les closures** : RFC séparée, après le modèle à messages (UI-D2).
- **Les traits et `dyn`** : les widgets sont un enum fermé fourni par
  `ravum-ui`, et l'extension par plugins est traitée à l'étape U6.
- L'API UI elle-même (`spec/ui/`) : étape U2.

## 5. Alternatives écartées

| Alternative | Pourquoi écartée |
|---|---|
| Durées de vie explicites (`&'a T`) | Change tout le modèle OWN-6 et coûte cher aux humains comme aux agents ; les références limitées suffisent pour l'UI |
| Faire parcourir l'arbre par le runtime Rust seulement, sans `match` sur référence | Chaque application ne pourrait plus inspecter sa propre vue (tests, Verify) : dette immédiate |
| `HashMap` | L'ordre de parcours n'est pas déterministe, ce qui contredit la vérification déterministe |
| Indices en caractères pour le texte | Incohérent avec `len()` en octets (STD-9.2) ; deux unités de mesure, c'est une source d'erreurs |

## 6. Preuve exigée avant de dire « implémenté »

Pour chaque proposition :
- la spécification mise à jour, avec des règles numérotées et des exemples
  positifs et négatifs ;
- des cas de conformance positifs (`run-pass`) et négatifs (`compile-fail`,
  avec le code exact) ;
- l'**accord des deux voies** : `rustc` et IR/JIT, vérifié par
  `ir_corpus_oracle`, ou bien un refus explicite et documenté de la voie C ;
- aucun test existant affaibli ; clippy, fmt et toute la conformance au vert.

## 7. Ordre de réalisation proposé

P1, puis P2 (même mécanisme de référence limitée), puis P5, puis P3 et P4.
Chaque proposition fait l'objet de sa propre PR vers `dev`.

## 8. Risques

- P1 et P2 touchent le vérificateur d'ownership, qui est la partie la plus
  sensible : il faut beaucoup de tests négatifs, et le fuzzing existant doit
  être étendu.
- P3 ajoute un type générique de prélude : la voie C doit le représenter
  (handles de tas, IR 1.6 probable).
- Rétrocompatibilité : tous les programmes Core 1.0 valides le restent, car
  les propositions n'acceptent que des formes aujourd'hui refusées.
