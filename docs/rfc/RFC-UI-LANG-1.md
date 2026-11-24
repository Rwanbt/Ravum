# RFC-UI-LANG-1 — Prérequis du langage pour Ravum UI

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-UI-LANG-1` |
| Statut | **PROPOSED** (révision 2), à décider par le responsable |
| Date | 2026-11-24 |
| Auteur | agent de développement |
| Cadre | [`DECISIONS-2026-11-24-UI.md`](../decision/DECISIONS-2026-11-24-UI.md), étape U1 de [`docs/ui/PLAN.md`](../ui/PLAN.md) |
| Portée | `spec/language/` (ownership, expressions, stdlib), `ravum-lang-sema`, `ravum-lang-codegen`, `ravum-lang-ir`/`ravum-lang-jit`, conformance |

La révision 2 intègre une revue externe du 2026-11-24 :
- elle corrige la contradiction entre `Option<&V>` et OWN-6.1 ;
- elle précise les emprunts bornés ;
- elle sépare les primitives de texte du moteur de texte de l'UI ;
- elle exclut les ressources du clone.

## 1. Contexte

Ravum UI suit un modèle à messages (UI-D2) :
- `view(&Model) -> Node<Msg>` construit un arbre ;
- `update(&mut Model, Msg)` modifie l'état ;
- le runtime parcourt l'arbre pour la mise en page, le rendu, la vérification et le routage des événements.

Ce que Core 1.0 accepte déjà, vérifié le 2026-11-24 :
- les enums génériques récursifs (`enum Node<M> { Leaf(M), Many(Vec<Node<M>>) }`) ;
- `match` **par valeur** ;
- la construction d'arbres et `&mut Model`.

Ce qui manque, constaté sur des programmes réels ([`V2-TRIAL-APPS.md`](../reports/V2-TRIAL-APPS.md) et un prototype d'arbre de nœuds) :
- lire une structure sans la consommer ;
- une table associative ;
- découper du texte ;
- copier des valeurs non `Copy`.

Ces ajouts servent tout le langage, et pas seulement l'UI.

## 2. Principe commun : les emprunts bornés

Core 1.0 connaît déjà une forme de référence non échappante, la **vue
d'instruction** (OWN-6.3, OWN-6.4, OWN-6.7 : `s.as_str()` est utilisable dans
l'instruction et jamais stockable). Cette RFC **étend explicitement** OWN-6
avec un seul nouveau concept, dérivé de ce précédent :

> **Emprunt borné** : une référence créée par une construction (un bras de
> `match`, un corps de `for`, ou une vue d'instruction) et qui expire à la
> fin de cette construction. Elle n'est jamais stockée (variable, champ,
> `Vec`, tuple, `Option` ou `Result` construits par l'utilisateur), jamais
> retournée et jamais capturée.

Il s'agit d'un **amendement normatif d'OWN-6** (nouvelle règle OWN-6.8), et
non d'une exception implicite. Il n'introduit aucune durée de vie explicite
et ne modifie aucun programme Core 1.0 valide : il n'accepte que des formes
aujourd'hui refusées.

Règles communes, MUST, numérotées OWN-6.8.x dans la spec à venir :
1. Pendant un emprunt borné **partagé** de `x`, toute écriture sur `x` ou
   sur une partie de `x`, et tout déplacement de `x`, sont refusés
   (`E-OWN-BORROW-CONFLICT`).
2. Pendant un emprunt borné **mutable** de `x`, tout autre accès à `x` est
   refusé (même code).
3. L'emprunt prend fin sur **tous** les chemins qui sortent de la
   construction : fin normale, `break`, `continue`, `return`, `?` et panic.
   Aucun état d'emprunt ne survit à la sortie.
4. Les emprunts imbriqués (un `match` dans un `for`, et le contraire)
   s'empilent ; l'emprunt intérieur expire avant l'extérieur.
5. Déplacer une valeur à travers un emprunt borné est refusé
   (`E-OWN-MOVE-OUT-OF-BORROW`) ; seul un champ `Copy` peut être lu par `*x`.
6. Un emprunt borné ne traverse jamais un point de remplacement à chaud.
   Le moteur ne remplace une fonction qu'entre deux appels (T16), donc
   aucune référence ne pointe vers une version retirée. Cette garantie doit
   être testée, et non supposée.

## 3. Propositions

### P1 — `match` sur une référence

Sur un scrutinee de type `&E` (ou `&mut E`), les liaisons d'un motif sont
des emprunts bornés au bras : `&T` (ou `&mut T`). Les paramètres de type de
l'enum sont **substitués** : sur `&Node<Msg>`, `Node::Many(kids)` lie
`kids: &Vec<Node<Msg>>`. Les motifs imbriqués propagent la référence
(`Node::Pair(Node::Leaf(a), _)` lie `a: &M`). Les gardes `if` lisent les
liaisons sans prolonger l'emprunt.

```rv
fn count(n: &Node<Msg>) -> i64 {
    match n {
        Node::Leaf(_) => 1,
        Node::Many(kids) => {
            let mut s: i64 = 1;
            for k in kids { s += count(k); }   // P2
            s
        }
    }
}
```

Cas négatifs, un cas de conformance chacun :
- `let keep = kids;` donne `E-OWN-REF-ESCAPE` ;
- `return kids` donne la même erreur ;
- `Some(kids)` donne la même erreur ;
- `let v = *kids;` (non `Copy`) donne `E-OWN-MOVE-OUT-OF-BORROW` ;
- sur `&mut`, `n` utilisé dans le bras donne `E-OWN-BORROW-CONFLICT`.

### P2 — `for x in &v` et `for x in &mut v`

`x` est un emprunt borné au corps, `&T` ou `&mut T`. `v` est emprunté pendant
toute la boucle : `v.push(..)`, `v.clear()` ou `v = ..` dans le corps donnent
`E-OWN-BORROW-CONFLICT`. Ceci exclut par construction l'invalidation
d'éléments pendant le parcours. Les boucles imbriquées sur le même `v` sont
admises en lecture et refusées si l'une est mutable.

### P3 — Table associative ordonnée `Map<K, V>`

Clés admises : entiers, `bool`, `char`, `String`. L'ordre total sur les
clés est fixé par la spec, et non par une bibliothèque :
- ordre numérique pour les entiers ;
- `false < true` ;
- ordre des points de code pour `char` ;
- ordre lexicographique des octets UTF-8 pour `String`.

Égalité : égalité de valeur, de contenu pour le texte (TYP-7.3).

| Opération | Sémantique | Complexité exigée |
|---|---|---|
| `Map::new()` | vide | O(1) |
| `len()`, `is_empty()` | taille | O(1) |
| `insert(k, v) -> Option<V>` | insère ou remplace, rend l'ancienne valeur | O(log n) |
| `remove(&k) -> Option<V>` | retire | O(log n) |
| `contains_key(&k) -> bool` | présence | O(log n) |
| `get(&k)` | **vue d'instruction** `Option<&V>` (voir ci-dessous) | O(log n) |
| `get_mut(&k)` | vue d'instruction `Option<&mut V>` | O(log n) |
| `for (k, v) in &m` | emprunts bornés, ordre croissant des clés | O(n) au total |
| `for (k, v) in &mut m` | `v: &mut V` ; les clés ne sont jamais modifiables | O(n) au total |
| `clone()` | si `V` est clonable (P5) | O(n) |

**Résolution de la contradiction relevée en revue.** `get` ne rend pas un
`Option<&V>` stockable : il produit une **vue d'instruction**, exactement comme
`as_str()` (OWN-6.3). Elle peut être :
- l'objet d'un `match`, dont les liaisons deviennent des emprunts bornés au bras (P1) ;
- testée (`.is_some()`) ;
- lue jusqu'à un `Copy` (`m.get(&k).unwrap_or(&0)` donne une copie si `V` est `Copy`).

Elle ne peut pas être stockée (`let o = m.get(&k);` donne
`E-OWN-REF-ESCAPE`, au titre d'OWN-6.1). OWN-6.1 reste donc vrai pour tout
`Option` **construit par l'utilisateur**, et la vue d'accès est nommée et
bornée par OWN-6.8. Aucune copie n'est imposée.

Accès par une clé absente : `get` rend `None`, sans panic. Il n'y a pas
d'indexation `m[k]`, qui paniquerait.

Représentation :
- voie B : `BTreeMap` de Rust, qui respecte l'ordre spécifié pour ces types de clés ;
- voie C : un objet de tas opaque de l'IR, à spécifier en IR 1.6.

La spec ne mentionne aucune bibliothèque.

### P4 — Primitives de texte (langage) et texte de l'UI (séparés)

**Langage** (`String` et `&str`), positions en octets UTF-8 comme `len()`
(STD-9.2) :

| Opération | Règle |
|---|---|
| `find(s) -> Option<usize>` | première occurrence ; `s` vide rend `Some(0)` |
| `split(sep) -> Vec<String>` | comme Rust : `"a,,b"` donne `["a","","b"]`, `""` donne `[""]` ; `sep` vide fait paniquer |
| `join(parts: &Vec<String>, sep: &str) -> String` | fonction de `std::string` |
| `strip_prefix(s)`, `strip_suffix(s) -> Option<String>` | `None` si absent |
| `substring(start, end) -> String` | panique si `start > end`, `end > len()` ou si une borne n'est pas une frontière de caractère |
| `chars() -> Vec<char>` | points de code Unicode |
| `is_char_boundary(i) -> bool` | pour valider une position avant `substring` |

Les versions **sans copie** (vues d'instruction sur une sous-chaîne) sont
possibles avec OWN-6.8. Elles feront l'objet d'une RFC dédiée si les mesures
montrent que les copies coûtent sur un chemin chaud.

**UI (hors de cette RFC, étape U3 et suivantes)** : l'édition de texte
(curseur par **graphème**, sélection, IME et composition, texte
bidirectionnel, polices de secours, shaping) relève du moteur de texte de
Ravum UI, et non de `Vec<char>`. Les primitives ci-dessus ne sont pas
destinées à l'éditeur.

### P5 — `clone()` structurel explicite

`clone()` est défini sur toute struct, tout enum, tout `Vec`, tout `Map`,
tout `Option` et tout `Result` dont tous les composants sont clonables,
récursivement. `String` et tous les types `Copy` sont clonables.

**Non clonables** : tout type que `std` ou le runtime déclare *ressource*,
c'est-à-dire les handles de service, de fenêtre ou de surface GPU, les
verrous et les sockets. Un type qui en contient un n'est pas clonable, et
`.clone()` donne `E-RES-UNKNOWN` avec une note qui nomme le champ fautif.

Le clone n'est **jamais implicite**, et le runtime UI ne clone pas l'arbre à
chaque événement : il route une **copie du seul message** concerné.

## 4. Hors périmètre de cette RFC, avec un jalon explicite

| Capacité | Jalon | Remarque |
|---|---|---|
| Handles typés générationnels | `RFC-UI-LANG-2`, avant U5 (composants) | type de bibliothèque `Handle<T>` (indice et génération), pas une référence ; un handle périmé rend `None` |
| Closures sans capture | `RFC-UI-LANG-3` | des valeurs de fonction, pour les commandes et les tâches |
| Closures avec capture | `RFC-UI-LANG-4` | des règles d'ownership explicites, qui dépendent de la RFC 3 |
| Traits statiques | `RFC-UI-LANG-5` | abstractions réutilisables |
| Dispatch dynamique, références stockables, ABI de composants | évaluées sur besoin démontré | aucune durée de vie générale ni ABI stable prématurées |

## 5. Alternatives écartées

| Alternative | Pourquoi écartée |
|---|---|
| Durées de vie explicites (`&'a T`) | Change tout le modèle OWN-6 et coûte cher aux humains comme aux agents ; les emprunts bornés suffisent |
| `get` par copie ou clone seulement | Copies coûteuses sur de grandes valeurs, donc de la dette ; la vue d'instruction existe déjà (OWN-6.3) |
| `HashMap` | Ordre de parcours non déterministe, contraire à la vérification déterministe |
| Positions en caractères | Incohérent avec `len()` en octets (STD-9.2) |
| Éditeur de texte sur `Vec<char>` | Faux pour les graphèmes, l'IME et le texte bidirectionnel ; relève du moteur de texte de l'UI |

## 6. Preuve exigée pour chaque proposition

1. La spec mise à jour : règles numérotées et FeatureIds, exemples positifs et négatifs.
2. Des cas de conformance positifs, négatifs (code exact) et adversariaux :
   tentatives d'échappement, emprunts imbriqués, sorties anticipées, et un
   `?` à l'intérieur d'un emprunt.
3. L'extension du fuzzing d'ownership existant aux nouvelles formes.
4. L'**accord des voies `rustc` et IR/JIT** sur tout le périmètre revendiqué
   (`ir_corpus_oracle`). Une forme manquante dans la voie C est un **défaut
   bloquant** pour Ravum UI, qui s'exécute à chaud par la voie C. Elle n'est
   pas une limite acceptable.
5. Des mesures si un chemin chaud est touché (`Map`, `split`).
6. Aucun test existant affaibli ; clippy, fmt et toute la conformance au vert.

## 7. Ordre de réalisation

P1 et P2 forment un seul mécanisme, OWN-6.8, livré dans une première PR.
Viennent ensuite P5, puis P3 (qui dépend de P1 pour `get`), puis P4. Une PR
par étape, vers `dev`.

## 8. Décisions demandées au responsable

1. **Ratifier OWN-6.8 (emprunts bornés)** comme amendement normatif
   d'OWN-6 (§2). C'est la décision de sémantique principale.
2. Ratifier l'ordre des clés de `Map` (§P3) et l'API `get` en vue
   d'instruction.
3. Ratifier la sémantique de `split` (identique à Rust, chaînes vides
   comprises) et des panics de `substring`.
4. Ratifier la liste des types non clonables (§P5).
