# Ravum Core 1.0 — Propriété, emprunts et destruction

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.7 (intégral), §3.2, §3.8.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR**. Aucune implémentation n'est revendiquée (`NOT_IMPLEMENTED`).
> **Aucune preuve de sûreté n'est revendiquée ici.** Le modèle est *conservateur par construction* au sens où il n'admet que des constructions dont le backend Rust émet un équivalent sûr ; ce n'est pas un théorème, c'est un argument de conception à vérifier par la conformance négative (gate T4).

## 1. Position du problème et choix conservateur

Le design §3.7 définit un sous-ensemble **plus restrictif que Rust** : pas de lifetimes explicites, pas de références stockées ni retournées, pas de `unsafe`. Ce document fixe les règles exactes, leur ordre de vérification, les codes émis, et **un exemple positif et un exemple négatif par règle**.

Trois notions sont utilisées partout :

- **état d'une place** ∈ { `UNINIT`, `INIT`, `MOVED`, `PARTIAL` } (§2) ;
- **prêt** (*loan*) : création d'un emprunt `&place` ou `&mut place` ; sa durée est **l'instruction englobante** (§3) ;
- **instruction** : une `let_stmt`, une `assign_stmt`, une expression-statement (`expr ;`), un `return_stmt`, un `break_stmt`, un `continue_stmt`, ou la *tail_expr* d'un bloc. Un appel est **contenu** dans une instruction : les prêts créés par ses arguments expirent à la fin de l'appel.

Le vérificateur d'ownership est **flow-sensitive mais volontairement simple** : il propage les états sur les branches (`if`, `match`) et les boucles, en étant **conservateur sur les fusions** (si un chemin a déplacé, le résultat est « peut-être déplacé », donc erreur à l'usage).

## 2. Classification Copy / mobile

| ID | Règle | Niveau |
|---|---|---|
| OWN-2.1 | Sont `Copy` : les primitifs listés en [`types.md`](types.md) TYP-14.1, les références (y compris `&str`), les tuples/tableaux d'éléments `Copy`, les structs/enums dont **tous** les champs sont `Copy`. | MUST |
| OWN-2.2 | Tout le reste est **mobile** : `String`, `Vec<T>`, `Option<T>`/`Result<T, E>` si `T`/`E` mobile, toute struct/enum contenant au moins un champ mobile, `Error`. | MUST |
| OWN-2.3 | Lire une place `Copy` **copie** ; la place reste `INIT`. Lire une place mobile **déplace** ; la place devient `MOVED`. | MUST |
| OWN-2.4 | `Copy` est calculé structurellement et n'est **pas** déclarable par l'utilisateur (pas de trait, pas de `derive`). | MUST |
| OWN-2.5 | Une opération qui exige une copie d'une valeur mobile est refusée : `[e; n]` sur un mobile est `E-TYP-ARRAY-REPEAT`. C'est la seule construction exigeant `Copy` en Core 1.0. | MUST |

```rv
// POSITIF — Point est Copy (tous champs primitifs)
struct Point { x: f64, y: f64 }
fn f(p: Point) -> Point {
    let q = p;        // copie
    return p;         // p toujours utilisable
}
fn g(a: [i32; 3]) -> i32 { let b = a; return a[0] + b[0]; }
```

```rv
// NÉGATIF — E-OWN-USE-AFTER-MOVE (String est mobile)
fn h(s: String) -> usize {
    let t = s;        // déplacement
    return s.len();   // E-OWN-USE-AFTER-MOVE (span: `s`)
}

// NÉGATIF — E-TYP-ARRAY-REPEAT
fn i(s: String) { let a = [s; 2]; }
```

## 3. Prêts : portée, conflits, non-échappement

### 3.1 Création et expiration

| ID | Règle | Niveau |
|---|---|---|
| OWN-3.1 | `&place` crée un **prêt partagé** ; `&mut place` un **prêt exclusif**. Le prêt porte sur le **chemin de place** exact (`x`, `x.y`, `x[0]`). | MUST |
| OWN-3.2 | Un prêt vit **au plus tard jusqu'à la fin de l'instruction englobante** (design §3.7 règle 3). Il ne peut pas traverser une instruction, un `let`, ni un `return`. | MUST |
| OWN-3.3 | Un prêt n'est **jamais** stocké, ni retourné, ni placé dans un champ, un élément de collection, un tuple ou un `Option`/`Result` — sauf `&'static str` issu d'un littéral (OWN-4). | MUST |
| OWN-3.4 | Un prêt peut être **reborrowed** explicitement dans la même instruction : `&*r`, `&mut *r`. Le reborrow expire avec l'instruction. | MUST |
| OWN-3.5 | Un prêt d'une place **non initialisée** est `E-OWN-UNINIT` ; un prêt d'une place `MOVED` ou `PARTIAL` est `E-OWN-USE-AFTER-MOVE` / `E-OWN-PARTIAL-MOVE`. | MUST |
| OWN-3.6 | Il n'y a **aucune** règle de « deux phases » (two-phase borrows), aucune réordonnancement de prêts, aucune prolongation de vie implicite (pas de *temporary lifetime extension*). | MUST |

```rv
// POSITIF
fn f(mut p: Point) -> f64 {
    let n = (&p).x;          // prêt partagé, meurt à la fin de la `let`
    p.x = 2.0;               // écriture après expiration : OK
    let m = (&mut p).x;      // reborrow explicite d'une place mutable
    return n + m;
}
```

```rv
// NÉGATIF — E-OWN-REF-ESCAPE (prêt stocké)
fn g(p: Point) { let r = &p.x; io::println("x"); let q = r; }

// NÉGATIF — E-OWN-REF-ESCAPE (prêt retourné)
fn h(p: &Point) -> &f64 { return &p.x; }
```

### 3.2 Conflits d'emprunt

| ID | Règle | Niveau |
|---|---|---|
| OWN-3.7 | Pendant un prêt **exclusif** sur un chemin, sont interdits **dans la même instruction** : toute lecture, toute écriture, tout autre prêt (partagé ou exclusif) de ce chemin. | MUST |
| OWN-3.8 | Pendant des prêts **partagés** sur un chemin, sont interdits toute écriture et tout prêt exclusif de ce chemin ; les lectures et d'autres prêts partagés restent permis. | MUST |
| OWN-3.9 | `f(&mut x, &mut x)` est refusé ; `f(&x, &mut x)` est refusé ; `f(&x, &x)` est **accepté** ; `f(&mut x, &x)` est refusé. | MUST |
| OWN-3.10 | Les prêts qui ne se recouvrent pas (chemins disjoints du même agrégat) ne sont **pas** en conflit : `f(&mut p.x, &mut p.y)` est accepté. | MUST |
| OWN-3.11 | Déplacer un chemin pendant qu'il est prêté (ou prêter un chemin pendant qu'il est déplacé) est un conflit (`E-OWN-BORROW-CONFLICT`) : `f(&x, x)` est refusé. | MUST |
| OWN-3.12 | Le conflit est évalué **par instruction**, pas par région : `{ let a = &mut x; }` puis `{ let b = &mut x; }` est accepté (deux instructions distinctes). | MUST |

Code : `E-OWN-BORROW-CONFLICT` (span : le second emprunt, avec une note sur le premier).

```rv
// POSITIF
fn f(a: &mut i32, b: &mut i32) { *a += *b; }
fn g() {
    let mut x = 1;
    let mut y = 2;
    f(&mut x, &mut y);       // chemins disjoints
    let mut p = Point { x: 0.0, y: 0.0 };
    f2(&mut p.x, &mut p.y);  // champs disjoints : OK
    let z = &x; let w = &x;  // deux prêts partagés : OK
}
```

```rv
// NÉGATIF — E-OWN-BORROW-CONFLICT
fn h() {
    let mut x = 1;
    f(&mut x, &mut x);       // deux prêts exclusifs
}
// NÉGATIF — E-OWN-BORROW-CONFLICT
fn i() {
    let mut x = 1;
    f2(&x, &mut x);          // partagé + exclusif
}
// NÉGATIF — E-OWN-BORROW-CONFLICT (déplacement pendant prêt)
fn j() {
    let s = String::from("a");
    f3(&s, s);               // &s puis move de s
}
```

### 3.3 Paramètres `&T` / `&mut T`

| ID | Règle | Niveau |
|---|---|---|
| OWN-3.13 | Un paramètre `&T` est utilisable **dans tout le corps** : son prêt est lié à l'appel, pas à l'instruction courante. Il est `Copy`. | MUST |
| OWN-3.14 | Un paramètre `&mut T` est utilisable dans tout le corps, y compris en écriture par `*r = e` et par reborrow `&mut *r`. Il n'est **pas** `Copy` au sens d'OWN-2.1 pour la duplication, mais peut être reborrowed. | MUST |
| OWN-3.15 | Un paramètre `&T`/`&mut T` ne peut être **ni retourné**, ni stocké dans une variable/champ/collection (`E-OWN-REF-ESCAPE`). | MUST |
| OWN-3.16 | Un paramètre `&mut T` ne peut pas être **déplacé** (la valeur pointée non plus) : `let v = *r;` déplace `*r` et est `E-OWN-MOVE-OUT-OF-BORROW`. | MUST |
| OWN-3.17 | Un paramètre `&T` peut être passé à un autre paramètre `&T` (`f(g(s))`) : c'est une recopie de prêt, pas une sortie. | MUST |

Code : `E-OWN-REF-ESCAPE`, `E-OWN-MOVE-OUT-OF-BORROW`.

```rv
// POSITIF
fn compte(v: &Vec<i32>) -> usize { return v.len(); }
fn ajoute(v: &mut Vec<i32>, x: i32) { v.push(x); }
fn chaine(v: &Vec<i32>) -> usize { return compte(v); }   // &T passé à &T

// NÉGATIF — E-OWN-REF-ESCAPE (paramètre retourné)
fn mauvais(v: &Vec<i32>) -> &Vec<i32> { return v; }
// NÉGATIF — E-OWN-REF-ESCAPE (paramètre stocké)
fn stocke(v: &Vec<i32>) { let w = v; }
// NÉGATIF — E-OWN-MOVE-OUT-OF-BORROW (déplacement à travers un &mut)
fn vide(v: &mut Vec<i32>) -> Vec<i32> { let w = *v; return w; }
```

## 4. Initialisation

| ID | Règle | Niveau |
|---|---|---|
| OWN-4.1 | Une place `UNINIT` ne peut être **ni lue**, **ni empruntée**, **ni déplacée**, **ni indexée** : `E-OWN-UNINIT`. | MUST |
| OWN-4.2 | `let x: T;` laisse `x` `UNINIT` ; `let x: T = e;` l'initialise. `let mut x: T;` est valide (état `UNINIT`). | MUST |
| OWN-4.3 | Une lecture de `x` exige que `x` soit `INIT` sur **tous** les chemins d'exécution atteignant cette lecture (analyse d'initialisation définie). Un chemin non initialisé suffit à l'erreur. | MUST |
| OWN-4.4 | L'assignation `x = e;` initialise une place `UNINIT` et **ré-initialise** une place `MOVED` (elle n'est pas une « utilisation »). L'assignation d'une place `INIT` remplace la valeur (EXPR-9.2). | MUST |
| OWN-4.5 | Une place `PARTIAL` reste utilisable **champ par champ** : les champs encore `INIT` sont lisibles/empruntables, les champs `MOVED` ne le sont pas. La place redevient `INIT` quand tous ses champs sont `INIT`, ou par **ré-assignation totale** (`d = Doc { ... }`). | MUST |
| OWN-4.6 | Un paramètre est toujours `INIT` à l'entrée de la fonction ; `self` aussi. | MUST |
| OWN-4.7 | Une constante `const` est toujours `INIT` et immuable. | MUST |

```rv
// POSITIF
fn f(c: bool) -> i32 {
    let x: i32;
    if c { x = 1; } else { x = 2; }   // initialisé sur tous les chemins
    return x;
}
fn g() -> String {
    let mut s: String;
    s = String::from("a");
    s = String::from("b");            // remplacement
    return s;
}
```

```rv
// NÉGATIF — E-OWN-UNINIT (chemin non initialisé)
fn h(c: bool) -> i32 {
    let x: i32;
    if c { x = 1; }
    return x;                          // pas initialisé si !c
}
// NÉGATIF — E-OWN-UNINIT (lecture avant initialisation)
fn i() -> i32 { let x: i32; return x; }
// NÉGATIF — E-OWN-UNINIT (emprunt d'une non initialisée)
fn j() { let x: i32; let r = &x; }
```

## 5. Déplacement (move)

| ID | Règle | Niveau |
|---|---|---|
| OWN-5.1 | Utiliser une place `MOVED` — lecture, écriture (hors ré-initialisation), emprunt, indexation, passage en argument, retour — est `E-OWN-USE-AFTER-MOVE`. | MUST |
| OWN-5.2 | Déplacer deux fois la **même** place est `E-OWN-DOUBLE-MOVE` (et non `USE-AFTER-MOVE`) lorsqu'il n'y a **aucun usage** entre les deux déplacements ; le message nomme les deux déplacements. | MUST |
| OWN-5.3 | Le déplacement d'un **champ de struct** est **autorisé et suivi par champ** : `let t = d.titre;` marque `d.titre` comme `MOVED`, les autres champs restent utilisables, et `fn into_titre(self) -> String { self.titre }` est valide. L'usage du **struct entier** (`d`, `d` en argument, `d` retourné) après un déplacement partiel est `E-OWN-PARTIAL-MOVE`. | MUST |
| OWN-5.4 | Le déplacement d'un **élément de tuple** ou d'un **élément de tableau** suit la même règle par composant : l'élément déplacé devient `MOVED`, les autres restent lisibles, et l'usage de l'agrégat **entier** est `E-OWN-PARTIAL-MOVE`. Aucun pattern de struct n'existe (G-6.36) : la décomposition passe par les champs. | MUST |
| OWN-5.5 | Le déplacement d'un élément de `Vec` par indexation est **interdit** (`E-OWN-PARTIAL-MOVE`) : la longueur serait incohérente. Utiliser `.pop() -> Option<T>` ou une méthode sur `&mut self`. | MUST |
| OWN-5.6 | Le déplacement dans une **boucle** d'une valeur déclarée **hors** de la boucle est refusé : la deuxième itération utiliserait une place déplacée (`E-OWN-USE-AFTER-MOVE`, note « déplacement possible à l'itération précédente »). | MUST |
| OWN-5.7 | Après une fusion de branches, si **un** chemin a déplacé la place, la place résultante est considérée `MOVED` (conservateur) : l'usage après le `if`/`match` est refusé même si l'autre chemin ne déplace pas. | MUST |
| OWN-5.8 | Passer une valeur mobile à une fonction la déplace ; passer un `Copy` la copie. Un appel qui déplace plusieurs arguments les déplace dans l'ordre d'évaluation (gauche→droite). | MUST |
| OWN-5.9 | `match` sur une valeur mobile déplace le scrutinee (EXPR-5.10) ; les liaisons de pattern déplacent les champs correspondants. | MUST |
| OWN-5.10 | `for x in v` où `v: Vec<T>` **déplace** `v` (itération consommatrice) : `v` est `MOVED` après la boucle (`E-OWN-USE-AFTER-MOVE` si réutilisé). `for i in a..b` ne déplace rien. | MUST |

```rv
// POSITIF
fn f(d: Doc) -> usize { return d.titre.len(); }       // d consommé, utilisé avant
fn g(t: (String, i32)) -> String { return t.0; }       // t jamais réutilisé
fn h(a: [String; 2]) -> String { return a[1]; }        // agrégat jamais réutilisé
fn i() -> String {
    let mut s = String::from("a");
    let t = s;                    // déplace s
    s = String::from("b");        // ré-initialise s (autorisé)
    return s;
}
```

```rv
// NÉGATIF — E-OWN-USE-AFTER-MOVE
fn j(s: String) -> usize { let t = s; return s.len(); }

// NÉGATIF — E-OWN-DOUBLE-MOVE (aucun usage entre les deux)
fn k(s: String) { let a = s; let b = s; }

// POSITIF — move d'un champ de struct : autorisé, suivi par champ
fn l(d: Doc) -> String { return d.titre; }
fn into_titre(d: Doc) -> String { return d.titre; }

// NÉGATIF — E-OWN-PARTIAL-MOVE (struct entier réutilisé)
fn l2(d: Doc) -> Doc { let t = d.titre; return d; }

// NÉGATIF — E-OWN-PARTIAL-MOVE (tuple réutilisé)
fn m(t: (String, i32)) -> i32 { let s = t.0; return t.1; }

// NÉGATIF — E-OWN-PARTIAL-MOVE
fn n(v: Vec<String>) -> String { return v[0]; }

// NÉGATIF — E-OWN-USE-AFTER-MOVE (déplacement dans une boucle)
fn o(v: Vec<String>) {
    let s = String::from("x");
    for i in 0..v.len() { drop(s); }
}
```

## 6. Non-échappement des références

| ID | Règle | Niveau |
|---|---|---|
| OWN-6.1 | Une référence non-`&'static str` **ne peut pas** être stockée dans une variable (`let r = &x;`), un champ, un élément de `Vec`, un tuple, un `Option`/`Result`, ni retournée : `E-OWN-REF-ESCAPE`. | MUST |
| OWN-6.2 | **Exception unique** : `&'static str` (littéral de chaîne) est `Copy`, stockable, retournable, comparable, et utilisable comme champ de struct. | MUST |
| OWN-6.3 | `String::as_str()` produit une `&str` **non statique** : elle ne peut pas être stockée ni retournée (`E-OWN-REF-ESCAPE`), seulement utilisée dans l'instruction (appel, comparaison). | MUST |
| OWN-6.4 | Une référence peut apparaître comme **résultat intermédiaire** d'une expression d'une seule instruction : `let n = (&p).x;` est valide (le résultat est `Copy`), `let r = &p.x;` ne l'est pas. | MUST |
| OWN-6.5 | Une référence ne peut pas être placée dans une constante. | MUST |
| OWN-6.6 | Le diagnostic `E-OWN-REF-ESCAPE` porte sur le **site d'échappement** (la `let`, le `return`, le champ), avec une note sur la création du prêt. | MUST |
| OWN-6.7 | La coercition `&String → &str` ([`types.md`](types.md) TYP-13.1) crée une **vue** qui expire avec l'instruction : elle ne permet pas de stocker le texte emprunté (`E-OWN-REF-ESCAPE`). `s.as_str()` suit exactement la même règle. | MUST |

```rv
// POSITIF
fn f(mut s: String, p: &Point) -> f64 {
    let taille = s.len();                 // valeur Copy
    let x = p.x;                          // valeur Copy
    let ok = s.as_str() == "oui";         // &str utilisée dans l'instruction
    io::println(s.as_str());              // idem
    let l = "littéral";                   // &'static str stockable
    io::println(l);
    s.push_str("!");
    return x + taille as f64;
}
```

```rv
// NÉGATIF — E-OWN-REF-ESCAPE (stockage)
fn g(p: Point) { let r = &p.x; }
// NÉGATIF — E-OWN-REF-ESCAPE (stockage de as_str)
fn h(s: String) { let t = s.as_str(); }
// NÉGATIF — E-OWN-REF-ESCAPE (retour)
fn i(s: &String) -> &str { return s.as_str(); }
// NÉGATIF — E-OWN-REF-ESCAPE (dans un champ)
struct W { r: &i32 }
// NÉGATIF — E-OWN-REF-ESCAPE (dans un Vec)
fn j(n: &i32) { let v: Vec<&i32> = Vec::new(); v.push(n); }
```

## 7. Mutabilité et place d'assignation

| ID | Règle | Niveau |
|---|---|---|
| OWN-7.1 | Une écriture exige une place **mutable** : liaison `let mut`, paramètre `&mut` déréférencé, champ d'une place mutable, élément d'une place mutable. Sinon `E-OWN-IMMUTABLE`. | MUST |
| OWN-7.2 | La mutabilité ne se « propage » pas à travers une référence partagée : `(*r)` où `r: &T` est immuable. | MUST |
| OWN-7.3 | L'écriture d'un champ privé hors module est `E-RES-PRIVATE` (et non `E-OWN-IMMUTABLE`), la visibilité étant vérifiée d'abord. | MUST |
| OWN-7.4 | Un receveur `&mut self` exige une place mutable (TYP-3.2) ; un receveur temporaire est `E-OWN-IMMUTABLE`. | MUST |

```rv
// POSITIF
fn f(mut p: Point, r: &mut Point, mut v: Vec<i32>) { p.x = 1.0; r.x = 2.0; v[0] = 3; }
```

```rv
// NÉGATIF — E-OWN-IMMUTABLE
fn g(p: Point) { p.x = 1.0; }
// NÉGATIF — E-OWN-IMMUTABLE (via référence partagée)
fn h(r: &Point) { r.x = 1.0; }
```

## 8. Destruction et ordre de drop

| ID | Règle | Niveau |
|---|---|---|
| OWN-8.1 | Les valeurs sont détruites à la **fin du bloc** où elles sont déclarées, en **ordre inverse de déclaration** (design §3.7 règle 5). Les paramètres sont détruits **après** les locales du corps, en ordre inverse de déclaration (voir Q-OWN-8). | MUST |
| OWN-8.2 | Une valeur **déplacée** n'est **pas** détruite à son emplacement d'origine ; elle est détruite à l'endroit où elle est finalement possédée. | MUST |
| OWN-8.3 | Une valeur **remplacée** par assignation est détruite **immédiatement**, avant l'écriture de la nouvelle valeur (EXPR-9.2). | MUST |
| OWN-8.4 | Une valeur **ombreée** (`let x = ..; let x = ..;`) est détruite à la fin du bloc, pas au point d'ombrement : les deux valeurs sont détruites en ordre inverse de déclaration. | MUST |
| OWN-8.5 | Les valeurs d'un agrégat sont détruites champ par champ, dans l'ordre de déclaration des champs, après la destruction de l'agrégat lui-même (aucun effet observable en Core 1.0, voir OWN-9). | MUST |
| OWN-8.6 | Une place `UNINIT` ou `MOVED` n'est pas détruite à la fin du bloc ; une place `PARTIAL` ne détruit que les champs encore `INIT`. | MUST |
| OWN-8.7 | `return` détruit les locales du bloc courant avant de rendre la valeur ; `break`/`continue` détruisent les locales du corps de boucle ; un panic ne détruit **rien** (arrêt immédiat, Q-EXPR-1). | MUST |

```rv
// POSITIF
fn f() {
    let a = String::from("a");     // détruit en 3e
    let b = String::from("b");     // détruit en 2e
    {
        let c = String::from("c"); // détruit en 1er (fin du bloc interne)
    }
    let d = String::from("d");     // détruit en 1er du bloc externe
}
fn g() -> String {
    let s = String::from("a");
    return s;                       // s déplacé : pas détruit localement
}
```

```rv
// NÉGATIF (sémantique) — E-OWN-USE-AFTER-MOVE : un déplacé ne peut plus être utilisé
fn h() {
    let s = String::from("a");
    let t = s;
    // io::println(s.as_str());   // <- interdit (E-OWN-USE-AFTER-MOVE)
}
```

## 9. `drop()` et absence de destructeur utilisateur

| ID | Règle | Niveau |
|---|---|---|
| OWN-9.1 | `drop(x)` est un **intrinsèque** du prélude : il prend `x` par valeur, détruit la valeur immédiatement, et rend `()`. Type : `drop<T>(x: T)`. | MUST |
| OWN-9.2 | `drop(x)` sur un `Copy` est **sans effet** (la copie est détruite, l'original reste utilisable) — cohérent avec OWN-2.3. | MUST |
| OWN-9.3 | `drop(x)` sur un mobile **déplace** `x` : toute utilisation ultérieure de `x` est `E-OWN-USE-AFTER-MOVE`. | MUST |
| OWN-9.4 | `drop(x)` appelle **uniquement** la destruction structurelle (libération des tampons `String`/`Vec`) ; aucun code utilisateur ne s'exécute. | MUST |
| OWN-9.5 | Il n'existe **aucun** `impl Drop`, aucun `#[derive(Drop)]`, aucun destructeur personnalisé : `E-UNSUPPORTED-DROP-IMPL`. | MUST |
| OWN-9.6 | Conséquence : l'ordre de drop (OWN-8) n'est **pas observable** par un programme Core 1.0. Il est néanmoins spécifié pour ne pas contraindre une future extension. | MUST |

```rv
// POSITIF
fn f() {
    let s = String::from("a");
    drop(s);                    // destruction immédiate
    let n = 1;
    drop(n);                    // sans effet : i32 est Copy
    n = 2;                      // toujours utilisable
}
```

```rv
// NÉGATIF — E-OWN-USE-AFTER-MOVE
fn g() { let s = String::from("a"); drop(s); io::println(s.as_str()); }

// NÉGATIF — E-UNSUPPORTED-DROP-IMPL
impl Drop for Doc { fn drop(&mut self) { } }
```

## 10. `unsafe`, pointeurs bruts, aliasing

| ID | Règle | Niveau |
|---|---|---|
| OWN-10.1 | `unsafe`, les blocs `unsafe { }`, les pointeurs bruts (`*const T`, `*mut T`), `transmute`, `offset`, et toute opération non vérifiée sont **absents** : `E-UNSUPPORTED-UNSAFE`, `E-UNSUPPORTED-UNSAFE`. | MUST |
| OWN-10.2 | Il n'existe aucune frontière FFI : `extern`, `#[no_mangle]`, `libc` sont refusés (`E-UNSUPPORTED-EXTERN`). | MUST |
| OWN-10.3 | Le code émis par le backend n'utilise que des constructions Rust **sûres** ; aucune `unsafe` n'est générée. | MUST |
| OWN-10.4 | **Argument de conception (non prouvé)** : parce que (a) aucune référence ne survit à son instruction, (b) toute référence est exclusive ou partagée sans recouvrement, (c) aucun pointeur brut ni `unsafe` n'existe, un programme accepté ne peut pas produire d'aliasing non vérifié. Cette affirmation est un **argument**, pas un théorème ; elle ne doit jamais être publiée comme `PROVED`. | MUST |
| OWN-10.5 | Corollaire : il n'y a **aucun** moyen d'exprimer une référence pendante, un double-free ou un use-after-free au niveau source ; ces fautes ne peuvent venir que d'un bug du compilateur ou du runtime (voir [`abi.md`](abi.md) §6). | MUST |

```rv
// POSITIF — aucune unsafe n'est nécessaire
fn f(v: &mut Vec<i32>) { v.push(1); }
```

```rv
// NÉGATIF — E-UNSUPPORTED-UNSAFE
fn g() { unsafe { } }
// NÉGATIF — E-UNSUPPORTED-UNSAFE
fn h(p: *const i32) { }
// NÉGATIF — E-UNSUPPORTED-EXTERN
extern "C" { fn puts(s: *const u8) -> i32; }
```

## 11. Ordre de vérification (déterminisme des diagnostics)

L'ordre suivant est **normatif** pour que deux implémentations produisent les mêmes diagnostics sur le même programme :

1. `E-RES-*` (résolution des noms, visibilité, modules, imports) ;
2. `E-TYP-*` (types, arité, exhaustivité, casts, littéraux) ;
3. `E-OWN-UNINIT` (initialisation définie) ;
4. `E-OWN-IMMUTABLE` (mutabilité des places) ;
5. `E-OWN-USE-AFTER-MOVE` / `E-OWN-DOUBLE-MOVE` / `E-OWN-PARTIAL-MOVE` / `E-OWN-PARTIAL-MOVE` / `E-OWN-MOVE-OUT-OF-BORROW` ;
6. `E-OWN-BORROW-CONFLICT` ;
7. `E-OWN-REF-ESCAPE` ;
8. `E-EFF-*` ;
9. `E-MOD-*` (manifeste, lockfile).

| ID | Règle | Niveau |
|---|---|---|
| OWN-11.1 | Une erreur d'une phase précoce **empêche** les phases suivantes sur le même nœud ; le compilateur ne doit pas produire un diagnostic d'ownership sur un programme dont les types sont invalides. | MUST |
| OWN-11.2 | À l'intérieur d'une phase, les diagnostics sont triés par `(fichier, start_byte, code)` croissants — ordre **déterministe** (T3). | MUST |
| OWN-11.3 | Le nombre de diagnostics d'ownership par fonction est borné (budget déterministe) ; au-delà, le compilateur émet un diagnostic unique `E-PARSE-TOO-LARGE` et s'arrête pour cette fonction. | MUST |

## 12. Table de correspondance design → règles

| Règle du design §3.7 | Règles ici | Codes |
|---|---|---|
| 1. Initialisation | OWN-4.* | `E-OWN-UNINIT` |
| 2. Move / use-after-move / double move / partiel | OWN-5.* | `E-OWN-USE-AFTER-MOVE`, `E-OWN-DOUBLE-MOVE`, `E-OWN-PARTIAL-MOVE`, `E-OWN-PARTIAL-MOVE` |
| 3. Borrows et conflits | OWN-3.1–3.12 | `E-OWN-BORROW-CONFLICT` |
| 4. Non-échappement, params | OWN-3.13–3.17, OWN-6.* | `E-OWN-REF-ESCAPE`, `E-OWN-MOVE-OUT-OF-BORROW` |
| 5. Drop, ordre, `drop()` | OWN-8.*, OWN-9.* | `E-UNSUPPORTED-DROP-IMPL` |
| 6. unsafe / pointeurs bruts | OWN-10.* | `E-UNSUPPORTED-UNSAFE`, `E-UNSUPPORTED-UNSAFE`, `E-UNSUPPORTED-EXTERN` |

## 13. Questions ouvertes

- **Q-OWN-1 (résolue par le Lead)** — Le move partiel de **struct** est **autorisé et suivi par champ** (OWN-5.3) : `d.titre` devient `MOVED`, les autres champs restent utilisables, le struct entier inutilisable (`E-OWN-PARTIAL-MOVE`), une ré-assignation totale rétablit `INIT`. Pas de pattern de struct en Core 1.0. `into_titre(self) { self.titre }` est valide.
- **Q-OWN-2 (résolue par le Lead)** — Conséquence de Q-OWN-1 : les structs « à ressource » sont consommables champ par champ (`into_*`) et par reconstruction. L'absence de `Clone` utilisateur reste une limite ergonomique documentée (Q-TYP-6), non bloquante.
- **Q-OWN-3** — OWN-5.7 (fusion conservatrice) refuse `if c { drop(s) }` puis ré-initialisation conditionnelle. C'est sûr mais grossier. Une analyse plus fine (états conditionnels) serait plus permissive ; à décider plus tard.
- **Q-OWN-4** — OWN-3.10 : les champs disjoints du **même** struct sont-ils prêtables simultanément (`&mut p.x`, `&mut p.y`) ? Ce document dit oui (chemins disjoints). Le design ne le dit pas ; c'est une précision nécessaire.
- **Q-OWN-5** — Les prêts de **sous-chemins par indexation** (`&mut v[i]`) : autorisés ? `v[i]` est une place ; `f(&mut v[0], &mut v[1])` serait accepté par chemins disjoints, mais l'implémentation doit prouver `0 != 1` — impossible en général. Règle proposée : les places indexées par une expression **non littérale** sont traitées comme **non disjointes** (conflit prudemment). À ratifier.
- **Q-OWN-6** — `drop(x)` sur un `Copy` (OWN-9.2) est sans effet. C'est cohérent mais surprenant ; alternative : refuser (`E-TYP`). Le design ne tranche pas.
- **Q-OWN-7** — OWN-8.3 (destruction immédiate du remplacé) : visible seulement via une future extension (Drop utilisateur). Non observable aujourd'hui.
- **Q-OWN-8** — L'ordre de destruction des **paramètres** (après les locales, ordre inverse) n'est pas dans le design, qui ne parle que des blocs. Précision proposée, à ratifier.
- **Q-OWN-9** — OWN-11.1 : un programme avec erreurs de type ne produit **aucun** diagnostic d'ownership. C'est voulu (évite le bruit), mais un IDE peut souhaiter les deux ; à trancher pour le mode `--json`.
- **Q-OWN-10** — Le budget `E-PARSE-TOO-LARGE` (OWN-11.3) n'existe pas dans le design ; il est introduit pour borner le temps d'analyse (P0-H). Nombre exact à fixer (proposition : 200 diagnostics par fonction).
