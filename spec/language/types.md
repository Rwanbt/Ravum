# Ravum Core 1.0 — Types, valeurs, places et conversions

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.2, §3.7, §3.8.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR**. Aucune implémentation n'est revendiquée (`NOT_IMPLEMENTED`).
> Voir [`grammar.md`](grammar.md) pour la syntaxe des types, [`ownership.md`](ownership.md) pour Copy/move/drop, [`expressions.md`](expressions.md) pour les opérateurs, [`stdlib.md`](stdlib.md) pour les signatures des types du prélude.

## 1. Types primitifs et tailles

| Type | Taille (octets) | Valeurs | Remarque |
|---|---|---|---|
| `i8 i16 i32 i64 i128` | 1, 2, 4, 8, 16 | complément à deux | `i128` = 16 |
| `u8 u16 u32 u64 u128` | 1, 2, 4, 8, 16 | non signé | |
| `isize` | 4 ou 8 | signé | largeur du pointeur de la cible |
| `usize` | 4 ou 8 | non signé | largeur du pointeur de la cible |
| `f32` | 4 | IEEE-754 binaire32 | |
| `f64` | 8 | IEEE-754 binaire64 | défaut des flottants |
| `bool` | 1 | `true` \| `false` | représentation 0/1, jamais d'autre valeur |
| `char` | 4 | scalaire Unicode `U+0000..=U+10FFFF` hors surrogats | pas un octet |
| `()` | 0 | une seule valeur `()` | type unité |

| ID | Règle | Niveau |
|---|---|---|
| TYP-1.1 | Les tailles ci-dessus sont **garanties** par la spécification, y compris `i128`/`u128` = 16 et `f32`/`f64` = IEEE-754. | MUST |
| TYP-1.2 | `isize`/`usize` valent 4 octets sur une cible 32 bits et 8 sur une cible 64 bits. Aucune autre largeur n'existe. Un programme qui dépend de cette largeur doit être testé sur les deux (voir [`compatibility.md`](compatibility.md)). | MUST |
| TYP-1.3 | Ces tailles ne sont **pas observables** depuis un programme Core 1.0 : il n'y a ni `size_of`, ni `align_of`, ni pointeur, ni FFI, ni `transmute` (voir TYP-14). Elles servent à la portabilité et au raisonnement, pas à l'introspection. | MUST |
| TYP-1.4 | `char` contient un **scalaire Unicode**, jamais un octet UTF-8, jamais un surrogat. `'é'` est un `char` valide ; un `u8` ne se convertit pas implicitement en `char`. | MUST |
| TYP-1.5 | `bool` n'a que deux valeurs ; il n'existe **aucune** conversion implicite entier↔bool. | MUST |
| TYP-1.6 | `!` est le **type Never** : aucune valeur, aucune taille. C'est le type des expressions divergentes (`return`, `break`, `continue`, `panic`, `process::exit`) et il **coerce implicitement vers tout type attendu** (aucun `E-TYP-MISMATCH` n'est émis pour `!`). Il n'est ni constructible, ni paramètre, ni champ, ni élément de `Vec`, ni retour d'une fonction non divergente. | MUST |

```rv
// POSITIF
fn f(a: i8, b: i128, c: u64, d: f32, e: char, g: ()) { }

// NÉGATIF — E-TYP-MISMATCH (entier vers bool)
fn h(x: i32) -> bool { return x; }
```

## 2. Valeurs et places

| ID | Règle | Niveau |
|---|---|---|
| TYP-2.1 | Une **valeur** est ce qui est produit par une expression : un scalaire, un agrégat, une référence, ou la valeur d'une place lue. | MUST |
| TYP-2.2 | Une **place** est un emplacement adressable : variable, champ, élément indexé, ou déréférencement d'une référence. Seules les places peuvent être affectées, empruntées ou déplacées. | MUST |
| TYP-2.3 | Une place est **mutable** si elle est atteinte par une chaîne de projections depuis une liaison `let mut`, un paramètre `&mut T` (via `*`), ou un champ `pub` d'une place mutable. Sinon elle est immuable. | MUST |
| TYP-2.4 | Lire une place de type `Copy` copie la valeur ; lire une place de type mobile la **déplace** (sauf opération d'emprunt explicite `&place`). | MUST |
| TYP-2.5 | Une expression de type `()` en position de valeur n'est pas une place ; `()` ne se déplace pas. | MUST |

```rv
// POSITIF
fn f(mut p: Point, r: &mut Vec<i32>) {
    p.x = 1.0;          // place mutable via `let mut`
    p.y = 2.0;
    r.push(1);          // méthode `&mut self` sur une place mutable
    (*r)[0] = 5;        // écriture à travers un &mut (place dérivée)
}
```

```rv
// NÉGATIF — E-OWN-IMMUTABLE (place immuable)
fn g(p: Point) { p.x = 1.0; }

// NÉGATIF — E-OWN-PARTIAL-MOVE (déplacement hors d'un Vec)
fn h(v: Vec<String>) -> String { return v[0]; }

// NÉGATIF — E-OWN-USE-AFTER-MOVE : lire une place mobile la déplace
fn i(v: Vec<i32>) -> usize { let w = v; return v.len(); }
```

## 3. Mutabilité

| ID | Règle | Niveau |
|---|---|---|
| TYP-3.1 | `let x = e;` lie une place immuable. `let mut x = e;` lie une place mutable. La mutabilité est une propriété de la **liaison**, pas du type. | MUST |
| TYP-3.2 | Une méthode `&mut self` exige un receveur place mutable ; un receveur temporaire (`Point::new(..).set(..)`) est refusé (`E-OWN-IMMUTABLE`, voir Q-TYP-3). | MUST |
| TYP-3.3 | Une méthode `&self` accepte un receveur immuable ou mutable. | MUST |
| TYP-3.4 | L'ombrement (`let x = 1; let x = "a";`) est autorisé et crée une **nouvelle** place ; l'ancienne est détruite à la fin du bloc selon [`ownership.md`](ownership.md) OWN-8. | MUST |
| TYP-3.5 | Il n'existe **aucun** `const fn`, aucune variable globale mutable, aucun `static`. Une constante `const` est immuable par définition et son type est explicite. | MUST |

```rv
// POSITIF
fn f() {
    let mut n = 0;
    n = 1;
    let n = "texte";     // ombrement : nouvelle liaison, type différent
}

// NÉGATIF — E-OWN-IMMUTABLE
fn g() { let n = 0; n = 1; }
```

## 4. Tuples

| ID | Règle | Niveau |
|---|---|---|
| TYP-4.1 | `(T1, ..., Tn)` est un produit ordonné, `n ≥ 1`. Le tuple vide est `()` (type primitif). | MUST |
| TYP-4.2 | L'arité et les types sont fixes ; l'accès se fait par indice littéral `t.0`, `t.1` (pas de variable d'indice). | MUST |
| TYP-4.3 | Un tuple est `Copy` si **tous** ses éléments sont `Copy` ; sinon il est mobile et suit les règles de [`ownership.md`](ownership.md). | MUST |
| TYP-4.4 | Le déplacement d'un élément d'un tuple (`t.0`) est autorisé à condition que le tuple ne soit **plus jamais utilisé** ensuite (`E-OWN-PARTIAL-MOVE` sinon). | MUST |
| TYP-4.5 | Les noms de champs de tuple sont `0..n-1` ; `t.n` hors bornes est `E-TYP-FIELD`. | MUST |

```rv
// POSITIF
fn f(t: (String, i32)) -> String {
    return t.0;          // déplacement du champ 0 ; t inutilisé ensuite
}

// NÉGATIF — E-OWN-PARTIAL-MOVE (t réutilisé après déplacement partiel)
fn g(t: (String, i32)) -> i32 { let s = t.0; let n = t.1; return n + s.len() as i32; }
```

## 5. Tableaux et slices

| ID | Règle | Niveau |
|---|---|---|
| TYP-5.1 | `[T; N]` est un tableau de longueur `N` **fixe et connue à la compilation**, `N ≥ 1` (G-5.5). Tous les éléments ont le même type. | MUST |
| TYP-5.2 | `a[i]` exige `i: usize` (`E-TYP-INDEX` sinon) et provoque un **panic contrôlé** si `i ≥ N`. Aucun accès non vérifié n'existe. | MUST |
| TYP-5.3 | Un tableau est `Copy` si `T` est `Copy` ; sinon mobile. | MUST |
| TYP-5.4 | Le déplacement d'un élément de tableau est autorisé si le tableau n'est plus utilisé ensuite (même règle que TYP-4.4). | MUST |
| TYP-5.5 | `&[T]` est la **vue non possédée** d'une séquence. Elle s'obtient par la coercition implicite `&[T; N] → &[T]` en position d'argument (TYP-13.1) ou explicitement par `Vec::as_slice()` ([`stdlib.md`](stdlib.md) STD-10.7) ; aucune autre expression ne la produit (pas de slicing `&v[a..b]` : `E-UNSUPPORTED-RANGE`). Elle expose `len()`, `is_empty()` et l'indexation `s[i]`. | MUST |
| TYP-5.6 | Une `&[T]` ne peut être ni stockée dans une variable, ni retournée, ni placée dans un champ ou un `Vec` (OWN-6) : elle est utilisable dans l'instruction (argument, récepteur, indexation) et expire avec elle. | MUST |
| TYP-5.7 | Un tableau littéral `[e1, ..., en]` a la longueur du nombre d'éléments ; `[e; n]` exige `e: Copy` (`E-TYP-ARRAY-REPEAT` sinon) et `n` littéral > 0. | MUST |
| TYP-5.8 | Parcourir une `&[T]` ou un `Vec<T>` se fait par index (`for i in 0..s.len()`). `for x in v` (forme collection) consomme un `Vec<T>` **possédé**, jamais une `&[T]`. | MUST |

```rv
// POSITIF
fn somme(a: [i32; 4]) -> i32 {
    let mut s = 0;
    for i in 0..4 { s += a[i]; }
    return s;
}

// NÉGATIF — E-TYP-INDEX (indice non usize)
fn g(a: [i32; 4], i: i32) -> i32 { return a[i]; }
// NÉGATIF — E-UNSUPPORTED-RANGE
fn h(v: Vec<i32>) { let s = &v[0..2]; }
// NÉGATIF — E-TYP-ARRAY-REPEAT (répétition d'un non-Copy)
fn i() { let a = [String::from("x"); 3]; }
```

## 6. Références (sous-ensemble sans lifetimes)

| ID | Règle | Niveau |
|---|---|---|
| TYP-6.1 | `&T` est une référence partagée (lecture seule) ; `&mut T` une référence exclusive (lecture/écriture). Toute référence est **non nulle** et **toujours valide** par construction. | MUST |
| TYP-6.2 | Il n'existe **aucune** lifetime explicite, aucun `'a`, aucun `'static` nommable par l'utilisateur. Toute référence a une durée de vie **inférée et bornée** par le modèle de [`ownership.md`](ownership.md) OWN-5..OWN-7. | MUST |
| TYP-6.3 | `Type::Str` est le **texte non possédé**, qui n'existe que derrière une référence : les seuls types nommables sont `&str` et `&'static str` (littéraux). `str` seul n'est pas un type (`E-RES-UNKNOWN`). Un `&str` est `Copy`, stockable, retournable et utilisable comme champ : c'est la **seule** référence qui échappe à OWN-6. | MUST |
| TYP-6.4 | `&str` (paramètre de texte) est une référence à du texte UTF-8 valide. Elle est `Copy` et utilisable dans tout le corps de la fonction ; elle ne peut être ni stockée dans une variable issue d'un emprunt local, ni retournée, ni mise dans un champ (sauf si elle vient d'un littéral `&'static str`). | MUST |
| TYP-6.5 | `str` seul n'est **pas** un type (`E-RES-UNKNOWN`). `&mut str` n'existe pas. | MUST |
| TYP-6.6 | `&&T`, `&mut &T` existent syntaxiquement mais une référence ne peut pas être empruntée au-delà de l'instruction (OWN-5) ; en pratique elles ne servent qu'à un appel immédiat. | MUST |
| TYP-6.7 | Aucune référence ne peut être convertie en entier, ni construite depuis un entier. Aucun pointeur brut. | MUST |
| TYP-6.8 | `*r` (déréférencement) est une place ; `&*r` (reborrow) produit une référence partagée explicite ; `&mut *r` une référence exclusive explicite. Ces reborrows sont les **seules** conversions de référence admises (aucune coercition implicite `&mut T → &T`). | MUST |

```rv
// POSITIF
fn f(s: &str, v: &mut Vec<i32>, n: &i32) -> i32 {
    let local = "littéral";          // &'static str, Copy
    io::println(s);
    io::println(local);
    v.push(*n);                      // copie du i32 pointé
    let partage: &Vec<i32> = &*v;    // reborrow explicite
    return partage.len() as i32;
}

// NÉGATIF — E-OWN-REF-ESCAPE (référence stockée)
fn g(v: &Vec<i32>) { let r = &v[0]; let s = r; s = r; }

// NÉGATIF — E-OWN-REF-ESCAPE (référence retournée)
fn h(v: &Vec<i32>) -> &i32 { return &v[0]; }

// NÉGATIF — E-OWN-REF-ESCAPE (emprunt d'un temporaire stocké)
fn i(a: i32) { let s = &(a + 1); }

// NÉGATIF — E-RES-UNKNOWN (`str` seul)
fn j(s: str) { }
```

## 7. Opérateurs et types admis

| Opérateur | Types admis | Résultat |
|---|---|---|
| `+ - * / %` | deux entiers **du même type** ; deux flottants du même type | type des opérandes |
| `<< >>` | gauche entier, droite entier (type quelconque) | type de gauche |
| `& \| ^` | deux entiers du même type, ou deux `bool` | type des opérandes |
| `!` | `bool` (négation logique) ou entier (complément à deux) | type de l'opérande |
| unaire `-` | entiers signés, flottants | type de l'opérande |
| `== !=` | entiers, flottants, `bool`, `char`, `&str` | `bool` |
| `< <= > >=` | entiers, flottants, `char` | `bool` |
| `&& \|\|` | `bool` | `bool` |
| `as` | voir TYP-10 | type cible |

| ID | Règle | Niveau |
|---|---|---|
| TYP-7.1 | Les opérandes arithmétiques **MUST** avoir exactement le même type. Aucune promotion ni conversion implicite (`a: i32 + b: i64` → `E-TYP-MISMATCH`). | MUST |
| TYP-7.2 | `-x` sur un type non signé est `E-TYP-MISMATCH`. | MUST |
| TYP-7.3 | `==`/`!=` sont définis sur : tous les entiers, `f32`/`f64`, `bool`, `char`, et le **texte** — `String`, `&str` et tout **mélange** des deux (comparaison par contenu UTF-8, vue `&String → &str` implicite). Ils ne sont **pas** définis sur `Vec`, `Option`, `Result`, les tuples, les tableaux, les structs/enums utilisateur, ni sur un paramètre de type `T` (pas de trait `PartialEq`). Violation : `E-TYP-EQ`. | MUST |
| TYP-7.4 | `< <= > >=` ne sont **pas** définis sur `&str` ni sur `String` (pas d'`Ord` exposé). Violation : `E-TYP-CMP`. | MUST |
| TYP-7.5 | Aucune surcharge d'opérateur, aucune coercition d'opérande, aucune égalité structurelle profonde automatique. | MUST |
| TYP-7.6 | `%` sur flottants suit IEEE-754 (`fmod`) : signe du dividende, `NaN` si un opérande est `NaN` ; `%` par `0.0` donne `NaN`, jamais de panic. À l'inverse `%` entier par zéro **panic**. | MUST |

```rv
// POSITIF
fn f(a: i32, b: i32) -> bool { return (a == b) && (a < b); }
fn g(x: &str, y: &str) -> bool { return x == y; }

// NÉGATIF — E-TYP-MISMATCH (types différents)
fn h(a: i32, b: i64) -> i64 { return a + b; }
// NÉGATIF — E-TYP-EQ (égalité structurelle sur String)
fn i(a: String, b: String) -> bool { return a == b; }
// NÉGATIF — E-TYP-CMP (ordre sur &str)
fn j(a: &str, b: &str) -> bool { return a < b; }
// NÉGATIF — E-TYP-MISMATCH (négation d'un non signé)
fn k(a: u32) -> u32 { return -a; }
```

## 8. `String`

`String` est le type **possédé** de texte UTF-8 (design §3.2). Il est **mobile** et non `Copy`.

| ID | Règle | Niveau |
|---|---|---|
| TYP-8.1 | Un `String` contient **toujours** de l'UTF-8 valide. Il ne peut pas exister de `String` invalide, y compris après mutation (`push`, `push_str`). | MUST |
| TYP-8.2 | `s.len()` est le nombre d'**octets** UTF-8, pas le nombre de caractères. Aucune API de comptage de caractères n'est fournie en Core 1.0 (voir [`stdlib.md`](stdlib.md)). | MUST |
| TYP-8.3 | `String` n'est **pas** indexable : `s[0]` est `E-TYP-INDEX`. Accès au texte via `.as_str()` (référence non échappante) ou `.bytes()` n'existe pas. | MUST |
| TYP-8.4 | `String` n'est pas comparable avec `==` (TYP-7.3) ; comparer `s.as_str()` à une `&str`. | MUST |
| TYP-8.5 | La conversion `&str → String` est explicite (`String::from(s)`), jamais implicite. La conversion `String → &str` est explicite (`.as_str()`). | MUST |
| TYP-8.6 | Il n'y a **aucun** littéral de type `String` : `"x"` a le type `&'static str`. `let s: String = "x";` est `E-TYP-MISMATCH`. | MUST |
| TYP-8.7 | `String` n'a pas de `Drop` utilisateur ; sa destruction est celle du runtime (`DROP` interne, `E-UNSUPPORTED-DROP-IMPL` pour toute tentative d'implémentation). | MUST |

```rv
// POSITIF
fn f() -> usize {
    let mut s = String::from("héllo");
    s.push_str(" !");
    s.push('é');
    return s.len();          // octets UTF-8, pas caractères
}

// NÉGATIF — E-TYP-MISMATCH (littéral affecté à String)
fn g() { let s: String = "x"; }
// NÉGATIF — E-TYP-INDEX
fn h(s: String) -> u8 { return s[0]; }
// NÉGATIF — E-TYP-EQ
fn i(a: String) -> bool { return a == "x"; }
```

## 9. `Vec<T>`

| ID | Règle | Niveau |
|---|---|---|
| TYP-9.1 | `Vec<T>` est une séquence possédée, de longueur dynamique, à éléments contigus, mobile et non `Copy`, pour tout `T` (y compris non `Copy`). | MUST |
| TYP-9.2 | `v[i]` exige `i: usize` et **panic** si `i ≥ len`. Lire `v[i]` copie si `T: Copy`. | MUST |
| TYP-9.3 | Si `T` n'est pas `Copy`, lire `v[i]` en position de valeur est refusé (`E-OWN-PARTIAL-MOVE`) : il faut `.pop()` (qui rend `Option<T>`) ou emprunter par une méthode. Indexer une `&[T]` (`s[i]`) suit la même règle. | MUST |
| TYP-9.4 | `v.push(x)` prend `x` par valeur (move s'il est mobile) et exige un receveur `&mut self`. `v.pop()` rend `Option<T>` et exige `&mut self`. | MUST |
| TYP-9.5 | Aucune réallocation n'est observable sémantiquement, mais `push` peut allouer : effet `alloc` ([`effects.md`](effects.md)). | MUST |
| TYP-9.6 | Il n'existe ni `map`, ni itérateurs. La lecture se fait par index (`for i in 0..v.len()`), et `for x in v` consomme le `Vec` (forme collection, OWN-5.10) ; aucune autre itération. | MUST |
| TYP-9.7 | `Vec<Vec<T>>`, `Vec<Option<T>>` etc. sont autorisés : `Vec` est un type générique ordinaire, non récursif par lui-même. | MUST |
| TYP-9.8 | Un `Vec` vide a `len() == 0` et n'est pas une valeur « nulle » : il n'y a pas de `null` dans le langage. | MUST |
| TYP-9.9 | Indexer une `&[T]` (`s[i]`) est une **lecture** : copie si `T: Copy`, sinon `E-OWN-PARTIAL-MOVE`. Une `&[T]` n'offre que `len`, `is_empty` et `s[i]` (jamais `push`, `pop`, `clear`). | MUST |

```rv
// POSITIF
fn somme(v: &mut Vec<i32>) -> i32 {
    let mut s = 0;
    for i in 0..v.len() { s += v[i]; }
    while v.len() > 0 {
        match v.pop() { Option::Some(x) => { s += x; }, Option::None => { } }
    }
    return s;
}

// NÉGATIF — E-TYP-MISMATCH (indice i32)
fn g(v: Vec<i32>, i: i32) -> i32 { return v[i]; }
// NÉGATIF — E-OWN-PARTIAL-MOVE (T non Copy)
fn h(v: Vec<String>) -> String { return v[0]; }
```

## 10. `Option<T>` et `Result<T, E>`

| ID | Règle | Niveau |
|---|---|---|
| TYP-10.1 | `Option<T>` a exactement deux variantes : `Option::Some(T)` et `Option::None`. `Result<T, E>` a `Result::Ok(T)` et `Result::Err(E)`. Ces types et variantes sont fournis par le prélude (voir [`stdlib.md`](stdlib.md)). | MUST |
| TYP-10.2 | Les variantes s'écrivent **qualifiées** (`Option::Some(x)`, `Result::Ok(x)`) et, pour ces quatre constructeurs du prélude **seulement**, **non qualifiées** (`Some(x)`, `None`, `Ok(x)`, `Err(e)`). Aucune variante n'est importable par `use` ([`modules.md`](modules.md) MOD-3.5) ; les variantes d'enums utilisateur restent obligatoirement qualifiées (`E-RES-UNKNOWN`). | MUST |
| TYP-10.3 | `Option<T>` est `Copy` si `T` est `Copy` ; `Result<T, E>` est `Copy` si `T` et `E` sont `Copy`. Sinon mobiles. | MUST |
| TYP-10.4 | `match` sur `Option`/`Result` doit être exhaustif (`E-TYP-NONEXHAUSTIVE`). | MUST |
| TYP-10.5 | Aucune conversion implicite `T → Option<T>` ni `Option<T> → T`. `?` est la seule propagation, avec les règles de [`expressions.md`](expressions.md) EXPR-8. | MUST |
| TYP-10.6 | Aucun `unwrap` implicite ; `.unwrap()` est un panic explicite (`panic: called Option::unwrap on None`), jamais une erreur silencieuse. | MUST |
| TYP-10.7 | Il n'existe **aucun** type somme récursif sans indirection : `struct Node { next: Option<Node> }` est `E-TYP-RECURSIVE` (pas de `Box` en Core 1.0). Un type se contenant lui-même, directement ou via un agrégat non-indexé, est refusé. Un `Vec<T>` n'introduit pas de récursion de taille (indirection interne). | MUST |
| TYP-10.8 | `Result<T, E>` n'est pas `Copiable` si `E` ne l'est pas ; une erreur `Error` contient un `String` et est donc mobile. `?` déplace l'erreur. | MUST |

```rv
// POSITIF
fn demi(x: i32) -> Option<i32> { if x % 2 == 0 { return Option::Some(x / 2); } return Option::None; }

fn lit(p: &str) -> Result<String, Error> {
    let s = fs::read_to_string(p)?;      // propagation
    return Result::Ok(s);
}

fn total(o: Option<i32>) -> i32 {
    match o { Option::Some(x) => x, Option::None => 0 }
}

// NÉGATIF — E-TYP-NONEXHAUSTIVE
fn g(o: Option<i32>) -> i32 { match o { Option::Some(x) => x } }
// NÉGATIF — E-RES-UNKNOWN
fn h(o: Option<i32>) -> i32 { match o { Some(x) => x, None => 0 } }
// NÉGATIF — E-TYP-RECURSIVE
struct Node { next: Option<Node>, value: i32 }
```

## 11. Génériques sans bornes

| ID | Règle | Niveau |
|---|---|---|
| TYP-11.1 | Un paramètre de type `T` est **opaque** dans le corps d'une fonction générique : il ne peut être ni copié implicitement, ni comparé, ni utilisé avec un opérateur arithmétique, ni recevoir d'appel de méthode (aucune borne, donc aucune méthode résoluble). | MUST |
| TYP-11.2 | Les seules opérations permises sur une valeur de type `T` sont : la lier, la déplacer, la rendre, la stocker dans un agrégat générique (`Vec<T>`, tuple, `Option<T>`), et la passer à une fonction générique acceptant `T`. | MUST |
| TYP-11.3 | Le compilateur **monomorphise** : chaque instanciation concrète est vérifiée après substitution. Une erreur qui n'apparaît qu'après substitution est rapportée sur l'instanciation et sur l'appel (`E-TYP-MISMATCH`, note « dans l'instanciation de `f<i32>` »). | MUST |
| TYP-11.4 | L'arité des paramètres génériques doit correspondre exactement (`E-TYP-ARITY`). Aucune valeur par défaut de paramètre de type. | MUST |
| TYP-11.5 | Un `impl<T> Foo<T>` ne peut implémenter que des méthodes sur `Foo<T>` ; les méthodes ne peuvent pas ajouter de paramètres génériques non déclarés par l'`impl` (`E-TYP-ARITY`). | MUST |
| TYP-11.6 | Il n'y a **aucune** borne, aucun `where`, aucun `impl Trait`, aucun `dyn` (`E-UNSUPPORTED-TRAIT`, `-WHERE`, `-IMPL-TRAIT`, `-DYN`). Conséquence assumée : `fn dup<T>(x: T) -> (T, T)` est **impossible** en Core 1.0 (il faudrait `T: Copy`), voir Q-TYP-5. | MUST |
| TYP-11.7 | Aucune spécialisation, aucune résolution de méthode dépendant du type concret à l'intérieur d'un corps générique. | MUST |

```rv
// POSITIF
pub fn identite<T>(v: T) -> T { return v; }
pub fn premier<T>(v: Vec<T>) -> Option<T> { return v.pop(); }   // pop rend Option<T>
struct Boite<T> { pub contenu: T }
fn emballer<T>(x: T) -> Boite<T> { return Boite { contenu: x }; }

// NÉGATIF — E-TYP-MISMATCH (opérateur sur un type opaque)
fn double<T>(x: T) -> T { return x + x; }
// NÉGATIF — E-TYP-EQ (comparaison sur un type opaque)
fn meme<T>(a: T, b: T) -> bool { return a == b; }
// NÉGATIF — E-TYP-ARITY
fn f<T>(x: T) -> T { return x; }
fn g() { let y = f::<i32>(1); }   // aussi E-PARSE-UNEXPECTED
```

## 12. Layout observable

| ID | Règle | Niveau |
|---|---|---|
| TYP-12.1 | Core 1.0 **ne garantit aucun layout d'octets** pour les types composés (struct, enum, tuple, `Vec`, `String`, `Option`, `Result`). L'ordre des champs, le remplissage, le discriminant d'enum et la disposition en mémoire ne sont **pas** observables. | MUST |
| TYP-12.2 | Ce non-engagement est **sans conséquence** : aucun programme Core 1.0 ne peut observer un layout (pas de pointeur, pas de FFI, pas de `size_of`/`offset_of`, pas de `transmute`, pas d'`unsafe`, pas d'`as` entre un type composé et un entier). | MUST |
| TYP-12.3 | Garanties **comportementales** (et non d'octets) : les éléments d'un tableau et d'un `Vec` sont ordonnés et indexables par `0..len` ; les champs d'un tuple sont accessibles par indice ; `match` sur enum distingue exactement les variantes déclarées. | MUST |
| TYP-12.4 | Les attributs de représentation (`#[repr(C)]`, `#[repr(u8)]`) sont des attributs : `E-UNSUPPORTED-ATTR`. | MUST |
| TYP-12.5 | Il n'existe **aucune** ABI stable ni interopérable en Core 1.0 (voir [`abi.md`](abi.md)). | MUST |

```rv
// POSITIF — aucune observation de layout n'est possible
fn f(p: Point) -> f64 { return p.x; }        // accès de champ, pas d'offset

// NÉGATIF — E-UNSUPPORTED-ATTR
#[repr(C)]
struct P { x: i32 }
// NÉGATIF — E-RES-UNKNOWN (aucune fonction size_of)
fn g() -> usize { return size_of::<i32>(); }
```

## 13. Conversions

### 13.1 Aucune coercition implicite

| ID | Règle | Niveau |
|---|---|---|
| TYP-13.1 | Il n'existe **que deux** ajustements implicites, tous deux en position d'argument ou de récepteur : (a) `&[T; N] → &[T]` (vue de tableau en slice), (b) `&String → &str` (vue de texte possédé en texte emprunté). **Aucune coercition `&Vec<T> → &[T]`** : utiliser `Vec::as_slice()`. Aucune autre conversion n'est implicite : ni entre types numériques, ni `&mut T → &T`, ni `T → Option<T>`, ni texte → nombre, ni agrégat → agrégat. | MUST |
| TYP-13.2 | **Seule exception** : l'inférence du type d'un littéral. Un littéral entier prend le type imposé par le contexte, `i32` par défaut ; un littéral flottant prend `f32`/`f64` du contexte, `f64` par défaut. Un littéral qui ne tient pas dans le type inféré est `E-TYP-LITERAL-RANGE`. | MUST |
| TYP-13.3 | Une expression `if`/`match`/bloc doit avoir **un seul** type : toutes les branches doivent s'accorder sans conversion (`E-TYP-MISMATCH`). Un littéral dans une branche s'adapte au type des autres branches. | MUST |
| TYP-13.4 | Le typage des littéraux est **contextuel et immédiat** : un littéral entier prend le type attendu par son contexte **local** (annotation, paramètre, autre opérande déjà typé), `i32` par défaut ; un littéral flottant prend `f64` par défaut. Il n'y a **pas** d'unification rétroactive : `let mut i = 0;` est `i32` et ne s'adapte pas à un usage ultérieur (`i < v.len()` avec `usize` → `E-TYP-MISMATCH`). Les exemples annotent donc `i: usize`/`u32`. Voir Q-TYP-10. | MUST |

```rv
// POSITIF
fn f() -> i64 { let x: i64 = 1; return x + 2; }   // littéraux adaptés à i64
fn g() { let v: Vec<i32> = Vec::new(); }

// NÉGATIF — E-TYP-LITERAL-RANGE
fn h() -> u8 { return 300; }
// NÉGATIF — E-TYP-AMBIGUOUS (Vec::new() sans type)
fn i() { let v = Vec::new(); }
// NÉGATIF — E-TYP-MISMATCH (branches incohérentes)
fn j(c: bool) { let x = if c { 1 } else { "a" }; }
```

### 13.2 `as` — casts explicites

| Source → cible | Sémantique | Statut |
|---|---|---|
| entier → entier | troncature des bits de poids fort (réduction) ou extension avec zéros / signe (élargissement) ; complément à deux | défini |
| entier → flottant | arrondi au plus proche, égalité au pair (round-half-to-even) | défini |
| flottant → entier | **saturant** : `NaN → 0`, `+inf → MAX`, `-inf → MIN`, sinon troncature vers zéro | défini |
| `f32` ↔ `f64` | conversion IEEE (arrondi) ; `f64 → f32` peut produire `±inf` | défini |
| `bool → entier` | `true = 1`, `false = 0` | défini |
| `char → u32`/entier | point de code scalaire | défini |
| entier → `char` | **refusé** (`E-TYP-CAST`) | interdit |
| entier → `bool` | **refusé** (`E-TYP-CAST`) | interdit |
| `char → bool` | **refusé** | interdit |
| tout autre (agrégat, référence, `String`, type utilisateur, `T` générique) | **refusé** (`E-TYP-CAST`) | interdit |

| ID | Règle | Niveau |
|---|---|---|
| TYP-13.5 | `as` ne s'applique qu'aux paires du tableau ci-dessus. Toute autre paire est `E-TYP-CAST`. | MUST |
| TYP-13.6 | `as` est **total** sur les paires admises : aucune panique, aucun UB. Les cas limites (`NaN`, infinis, débordement) sont ceux du tableau. | MUST |
| TYP-13.7 | `as` se chaîne à gauche : `x as u8 as i64` = `(x as u8) as i64` (donc troncature puis extension). | MUST |
| TYP-13.8 | `as` ne s'applique **jamais** à un type composé ni à une référence ; il n'existe aucun `as` de conversion de texte. Les conversions texte passent par [`stdlib.md`](stdlib.md) (`parse_*`, `*_to_string`). | MUST |
| TYP-13.9 | Un `as` vers le même type est accepté et sans effet (`x as i32` où `x: i32`). | MUST |

```rv
// POSITIF
fn f(a: i32, b: f64) -> u8 { return a as u8; }
fn g(x: f64) -> i32 { return x as i32; }      // saturant, NaN -> 0
fn h(c: char) -> u32 { return c as u32; }
fn i(b: bool) -> u8 { return b as u8; }

// NÉGATIF — E-TYP-CAST (entier vers char)
fn j(n: u32) -> char { return n as char; }
// NÉGATIF — E-TYP-CAST (référence)
fn k(v: &Vec<i32>) -> usize { return v as usize; }
// NÉGATIF — E-TYP-CAST (String)
fn l(s: String) -> u64 { return s as u64; }
```

## 14. Copy

| ID | Règle | Niveau |
|---|---|---|
| TYP-14.1 | Sont `Copy` : tous les primitifs (`i* u* f* bool char ()`), les références (`&T`, `&mut T`, `&str`), les tuples et tableaux dont tous les éléments sont `Copy`, les structs/enums dont **tous** les champs sont `Copy`. | MUST |
| TYP-14.2 | Sont **mobiles** (non `Copy`) : `String`, `Vec<T>`, `Option<T>`/`Result<T, E>` si `T`/`E` mobile, les structs/enums contenant au moins un champ mobile, `Error`. | MUST |
| TYP-14.3 | `Copy` n'est **pas** un trait utilisateur et ne se déclare pas : il est calculé structurellement. Il n'existe aucun `impl Copy`, aucun `#[derive(Copy)]`. | MUST |
| TYP-14.4 | Il n'existe **aucun** `Clone` utilisateur. Les types mobiles du prélude fournissent une méthode `.clone()` inhérente explicite (`String::clone`, `Vec::clone`) ; les types utilisateur n'ont **pas** de `.clone()` (`E-RES-UNKNOWN`) — la duplication se fait par reconstruction explicite, voir Q-TYP-6. | MUST |
| TYP-14.5 | Un type générique `T` n'est **pas** `Copy` par défaut : on ne peut pas copier une valeur `T` (TYP-11.1). | MUST |

```rv
// POSITIF
struct Point { x: f64, y: f64 }            // tous champs Copy => Point est Copy
fn f(p: Point) -> Point { let q = p; return p; }   // copie, p reste utilisable

// NÉGATIF — E-OWN-USE-AFTER-MOVE (struct mobile)
struct Doc { titre: String, n: i32 }
fn g(d: Doc) { let e = d; io::println(d.titre.as_str()); }
```

## 15. Questions ouvertes

- **Q-TYP-1 (résolue par le Lead)** — `&[T]` s'obtient par coercition `&[T; N] → &[T]` en argument ou par `Vec::as_slice()`. Il n'y a **aucune** coercition `&Vec<T> → &[T]` (TYP-13.1) : `as_slice()` est explicite, non stockable, utilisable dans l'instruction. Le slicing reste `E-UNSUPPORTED-RANGE`.
- **Q-TYP-2 (résolue par le Lead)** — La coercition `&[T; N] → &[T]` ne s'applique qu'en argument/récepteur ; dans une `let` (`let s: &[f64] = &input;`), c'est `E-OWN-REF-ESCAPE`.
- **Q-TYP-3** — TYP-3.2 refuse les méthodes `&mut self` sur un receveur temporaire. C'est une conséquence du modèle d'emprunt borné à l'instruction, mais c'est une restriction notable (`Point::new(..).set_x(1.0)`). Confirmer.
- **Q-TYP-4** — `[T; 0]` refusé (G-5.5). Utile pour des buffers génériques ; à confirmer.
- **Q-TYP-5** — Sans bornes, `fn dup<T>(x: T) -> (T, T)` et tout code générique copiant une valeur sont impossibles. C'est cohérent avec le design (« génériques sans bornes ») mais limite fortement l'usage. Faut-il un `Copy` magique (implicite pour les types Copy) sans trait ? Cela introduirait une règle non prévue par le design : **décision humaine requise**.
- **Q-TYP-6** — Absence de `Clone` utilisateur (TYP-14.4) : dupliquer un `struct Doc { String }` demande de reconstruire chaque champ. Faut-il un `impl Doc { fn clone(&self) -> Doc }` **manuel** (possible aujourd'hui) ou un `#[derive(Clone)]` (impossible, attributs UNSUPPORTED) ? Le manuel fonctionne mais doit être écrit à la main ; à documenter dans les guides.
- **Q-TYP-7 (résolue par le Lead)** — L'égalité texte est étendue (TYP-7.3) : `String`/`&str` et mélanges. Les agrégats restent non comparables (`E-TYP-EQ`). Une égalité structurelle restreinte aux types scalaires reste une extension non retenue.
- **Q-TYP-8** — TYP-1.2 : `usize` dépend de la cible. Faut-il l'exclure des signatures publiques de la stdlib pour la portabilité ? En l'état `len()` rend `usize`, ce qui est indispensable.
- **Q-TYP-9** — L'arrondi entier→flottant (round-half-to-even) est celui de Rust ; il doit être vérifié par la conformance sur des valeurs exactes à mi-chemin (`2^53+1`).

- **Q-TYP-10 (limite assumée, décision Lead)** — Pas d'unification rétroactive (TYP-13.4) : `let mut i = 0;` est `i32` et ne peut pas servir d'indice `usize`. `for i in 0..v.len()` type `i` en `usize` et fonctionne ; `let mut i = 0; while i < v.len() { i += 1; }` est refusé sans annotation. Une unification HM-like est une extension future.
