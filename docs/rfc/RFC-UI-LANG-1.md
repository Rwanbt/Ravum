# RFC-UI-LANG-1 — Prérequis du langage pour Ravum UI

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-UI-LANG-1` |
| Statut | **PROPOSED** (révision 4). Le **principe** d'OWN-6.8 et de P1 à P5 est validé par le responsable (UI-D11) ; les règles normatives de §2.1 restent à ratifier avant l'implémentation définitive |
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
6. **Remplacement à chaud, y compris en multithread.** Un emprunt borné
   ne vit que dans une activation, et il ne passe jamais d'un thread à
   l'autre : une référence n'est ni stockable ni envoyable par un canal
   (OWN-6.1). La règle comporte deux points sûrs distincts :
   - **par thread** : un thread n'adopte une nouvelle version qu'à profondeur
     d'appel 0 de **son** exécution, entre deux appels d'entrée (T16) ;
   - **global** : le code et les données d'une version retirée ne sont
     libérés qu'une fois que **tous** les threads qui l'exécutaient sont
     passés par leur point sûr (récupération par génération, existante).
     Tant qu'un thread est dans l'ancienne version, elle reste valide.

   Il n'existe donc, à aucun instant, d'emprunt vivant vers une version
   libérée, ni direct ni transitif à travers des appels imbriqués. Le moteur
   doit le vérifier : refus d'adoption hors profondeur 0, et refus de
   libération tant qu'un thread n'a pas acquitté. Tests exigés :
   - remplacement pendant des appels imbriqués ;
   - plusieurs threads, dont un bloqué dans l'ancienne version ;
   - libération différée observée.
7. **Suspension asynchrone.** Un emprunt borné vivant à un `.await` est
   refusé (nouveau code `E-OWN-BORROW-ACROSS-AWAIT`), tant qu'aucune
   sémantique de suspension n'aura été qualifiée.

### 2.1 Typage et propagation des vues (règles à ratifier)

- **Provenance.** Le checker associe à chaque référence bornée sa
  **provenance** : la place empruntée (racine et chemin de champs) et la
  portée qui la borne. La provenance se **transmet** à travers les
  réemprunts (`&*x`, `&x.champ`), les liaisons de motif, les méthodes qui
  rendent une référence, et les appels dont le résultat est une référence
  issue d'un argument. La portée d'un résultat est la plus courte des
  portées de ses sources. Les conflits (règles 1 et 2) se calculent sur
  la provenance, et non sur le nom local.
- **Propagation des vues.** Seule une valeur qui **contient** une référence
  est bornée. `Option<&V>` rendu par `get` est borné, et `.unwrap()`, un
  `match` ou une liaison le restent. Une opération qui produit une valeur
  **sans** référence n'est pas bornée : `.is_some()` rend un `bool`, `*x`
  une copie `Copy`, `x.clone()` une valeur possédée, `x.len()` un entier.
  Le résultat est alors une valeur ordinaire, stockable.
- **Usages admis** : passage en argument (le paramètre de l'appelé est une
  référence ordinaire, qui ne peut déjà pas s'échapper au titre d'OWN-6.1),
  réemprunt (`&*x`, `&x.champ`), lecture de champ, comparaison, `match`
  (les liaisons héritent de la portée), et appel de méthode.
- **Usages refusés** (`E-OWN-REF-ESCAPE`) : `let`, champ, élément de `Vec`
  ou de `Map`, tuple, `Option` ou `Result` **construits par l'utilisateur**,
  `return`, valeur d'un bloc qui sort de la portée.
- **Accès par l'emprunt lui-même.** Pendant un emprunt mutable `x` de la
  place `p`, les lectures et écritures **à travers `x`** (`*x = ..`,
  `x.champ = ..`, méthodes mutantes sur `x`) sont permises. Seuls les accès
  à `p` par un **autre** chemin sont refusés (règle 2). Pendant un emprunt
  partagé, les lectures de `p` par tout chemin sont permises.
- **Accès disjoints** : deux emprunts de places disjointes (champs
  différents) suivent l'analyse de places existante du vérificateur
  d'ownership. Deux emprunts du même élément indexé `v[i]` et `v[j]` sont
  traités comme conflictuels (indices non comparés statiquement).
- **Sorties anticipées** : `break`, `continue`, `return` et `?` ferment
  toutes les portées traversées (règle 3).
- **Sémantique de `&mut` (normative, indépendante de l'implémentation).**
  Une écriture à travers un emprunt mutable est **immédiatement** celle de
  la place empruntée. Comme aucun autre chemin ne peut observer la place
  pendant l'emprunt (règle 2), seul l'état **à la fin** de l'emprunt est
  observable. Deux cas de sortie anormale :
  - un **panic** termine le programme (Core) : rien n'est observé ensuite ;
  - un **trap en temps réel** restaure tout l'état d'entrée avant le
    rollback (HR-1.5).

  Une implémentation par copie en entrée et écriture en retour est donc
  **admise si et seulement si** elle écrit en retour sur **toute** sortie
  normale, y compris `break`, `continue`, `return` et `?`. Cette équivalence
  doit être **testée** sur ces sorties, sur des emprunts imbriqués et sur
  les champs d'éléments de `Vec`, avec accord des voies B et C. Elle n'est
  pas supposée.
- **Représentation dans l'IR et le JIT.** Les emprunts bornés n'ont
  **aucun objet à l'exécution**. Ils se réduisent aux places déjà
  existantes de l'IR :
  - adresse (`Ptr`) pour la pile ;
  - élément de vecteur (handle et indice) ;
  - copie en entrée et écriture en retour pour `&mut`, sous la condition
    ci-dessus ;
  - pour `Map` : handle et position, à spécifier en IR 1.6.

  Les règles de conflit garantissent qu'aucune mutation n'invalide une place
  pendant sa portée.

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
- lue par **déréférencement explicite** quand `V` est `Copy` : `m.get(&k).unwrap_or(&0)` reste une référence bornée `&V` ; `*m.get(&k).unwrap_or(&0)` donne la valeur. Aucune copie implicite n'est faite.

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
| `split(sep) -> Vec<String>` | `"a,,b"` donne `["a","","b"]`, `""` donne `[""]` (comme Rust). **Écart volontaire avec Rust** : un `sep` vide **panique** (`panic: empty separator`). Rust coupe alors entre chaque caractère et ajoute deux chaînes vides aux bords, ce qui est une source d'erreurs ; `chars()` couvre ce besoin |
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

**Non clonables, par classification normative** : la clonabilité est une
**propriété de type**, et non une liste de noms. Chaque type de `std` ou du
runtime est déclaré dans sa spécification soit *valeur* (clonable si ses
composants le sont), soit *ressource* (jamais clonable) : handles de service,
de fenêtre ou de surface GPU, verrous, sockets, `Future`. La propriété se
propage structurellement : un type qui contient une ressource est une
ressource. `.clone()` sur une ressource donne `E-RES-UNKNOWN`, avec une note
qui nomme le champ fautif. En Core, un utilisateur ne peut pas déclarer un
type *ressource* ; la question sera rouverte avec les handles
(RFC-UI-LANG-2).

**Effets** : un `clone()` qui peut allouer (`String`, `Vec`, `Map`, ou un
type qui en contient) porte l'effet `alloc` (EFF-2.5, granularité
fonction). Il est donc refusé dans les profils `pure` et `rt`, comme toute
allocation. Un `clone()` de type entièrement `Copy` n'a aucun effet.

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
6. Les tests spécifiques de la révision 4 :
   - provenance transmise par réemprunt, motif, méthode et appel ;
   - accès à travers l'emprunt mutable permis, accès concurrent refusé ;
   - `bool`, `Copy` et clone issus d'une vue, qui sont stockables ;
   - équivalence de `&mut` sur `break`, `continue`, `return`, `?` et les emprunts imbriqués, pour les voies B et C ;
   - points sûrs multithreads et libération différée ;
   - `clone()` allouant refusé en `rt` et en `pure` ;
   - `split("")` qui panique.
6. Aucun test existant affaibli ; clippy, fmt et toute la conformance au vert.

## 7. Ordre de réalisation

P1 et P2 forment un seul mécanisme, OWN-6.8, livré dans une première PR.
Viennent ensuite P5, puis P3 (qui dépend de P1 pour `get`), puis P4. Une PR
par étape, vers `dev`.

## 8. Décisions demandées au responsable

Le principe de 1 à 4 a été validé le 2026-11-24 (UI-D11). Restent à ratifier les **règles normatives** rédigées dans la spec à l'implémentation : §2, §2.1, l'ordre des clés, la sémantique de `split` et de `substring`, et la classification valeur ou ressource.


1. **Ratifier OWN-6.8 (emprunts bornés)** comme amendement normatif
   d'OWN-6 (§2). C'est la décision de sémantique principale.
2. Ratifier l'ordre des clés de `Map` (§P3) et l'API `get` en vue
   d'instruction.
3. Ratifier la sémantique de `split` (identique à Rust, chaînes vides
   comprises) et des panics de `substring`.
4. Ratifier la liste des types non clonables (§P5).
