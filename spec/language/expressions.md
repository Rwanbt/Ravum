# Ravum Core 1.0 — Expressions, évaluation et contrôle

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.3, §3.4, §3.8.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR**. Aucune implémentation n'est revendiquée (`NOT_IMPLEMENTED`).
> La grammaire et la précédence sont dans [`grammar.md`](grammar.md) §6.2 ; les opérateurs et leurs types dans [`types.md`](types.md) §7 et §13 ; l'évaluation d'un point de vue machine dans [`core-machine.md`](core-machine.md).

## 1. Ordre d'évaluation

| ID | Règle | Niveau |
|---|---|---|
| EXPR-1.1 | L'évaluation est **de gauche à droite**, y compris pour les arguments d'appel, les éléments d'un agrégat, les champs d'un littéral de struct, et les opérandes binaires. | MUST |
| EXPR-1.2 | Pour `a + b`, `a` est évalué entièrement, puis `b`. Pour `f(a, b, c)`, l'expression désignant `f` est évaluée d'abord, puis `a`, `b`, `c`. | MUST |
| EXPR-1.3 | Pour `recv.m(args)`, `recv` est évalué **avant** `args`. Pour `a[i]`, `a` est évalué avant `i`. Pour `t.f`, `t` avant l'accès. | MUST |
| EXPR-1.4 | L'ordre est **observable** via les effets (E/S, `panic`) et via les emprunts ; il ne peut pas être réordonné par le compilateur ni par le backend. Un programme dont le résultat dépend de l'ordre est valide et doit produire le résultat spécifié. | MUST |
| EXPR-1.5 | Les opérandes d'un agrégat (`Point { x: e1, y: e2 }`, `(e1, e2)`, `[e1, e2]`) sont évalués dans l'**ordre textuel** de déclaration. | MUST |
| EXPR-1.6 | Un `let x = e;` évalue `e` avant d'initialiser `x`. Un `let x: T;` n'évalue rien et laisse `x` non initialisée (OWN-1). | MUST |
| EXPR-1.7 | Aucune évaluation partielle observable autre que par `panic` : si l'évaluation d'un opérande panic, les opérandes suivants ne sont **pas** évalués, et l'état observable est celui d'avant l'instruction (aucune écriture partielle de l'expression). | MUST |

```rv
// POSITIF — l'ordre est observable
fn f() { io::print("a"); io::print("b"); }        // affiche "ab"
fn g() -> i32 { io::print("g"); return 1; }
fn h() -> i32 { io::print("h"); return 2; }
fn somme() -> i32 { return g() + h(); }           // affiche "gh", rend 3
fn args() { f2(g(), h()); }                       // affiche "gh"

// NÉGATIF — E-EFF-UNDECLARED (si `g`/`h` sont déclarées effects())
fn g() -> i32 effects() { return 1; }             // corps sans effet : OK
```

## 2. Court-circuit

| ID | Règle | Niveau |
|---|---|---|
| EXPR-2.1 | `a && b` : `a` est évalué ; si `a == false`, `b` **n'est pas évalué** et le résultat est `false`. | MUST |
| EXPR-2.2 | `a \|\| b` : `a` est évalué ; si `a == true`, `b` **n'est pas évalué** et le résultat est `true`. | MUST |
| EXPR-2.3 | Les opérandes de `&&`/`\|\|` sont **exactement** `bool` ; aucun opérateur bit à bit n'est appliqué et aucune valeur « truthy » n'existe (`E-TYP-MISMATCH`). | MUST |
| EXPR-2.4 | Le court-circuit est **garanti** : il ne peut pas être transformé en évaluation stricte par une optimisation. Un programme qui dépend de l'absence d'effet de l'opérande droit est valide. | MUST |
| EXPR-2.5 | `&`, `\|`, `^` (bit à bit) **n'ont pas** de court-circuit et évaluent leurs deux opérandes. | MUST |

```rv
// POSITIF
fn f(v: &Vec<i32>, i: usize) -> bool {
    return i < v.len() && v[i] == 0;     // v[i] non évalué si i hors bornes
}

// NÉGATIF — E-TYP-MISMATCH (entier dans &&)
fn g(n: i32) -> bool { return n && true; }
```

## 3. Arithmétique entière : débordement, division, décalage

| ID | Règle | Niveau |
|---|---|---|
| EXPR-3.1 | Le débordement entier (`+ - *`, y compris `i32::MIN / -1` et `-i32::MIN`) provoque un **panic contrôlé** dans **tous** les profils, debug comme release (`-C overflow-checks=on`, design §3.3). Il n'existe **aucune** arithmétique enveloppante implicite. | MUST |
| EXPR-3.2 | La division entière par zéro et le reste par zéro provoquent un **panic contrôlé** (`panic: division by zero` / `remainder by zero`). | MUST |
| EXPR-3.3 | Un décalage dont la quantité est `>= largeur_bits(gauche)` provoque un **panic contrôlé**, y compris à droite. Les décalages ne sont **jamais** masqués (`& 31`) ni transformés en UB. | MUST |
| EXPR-3.4 | La quantité de décalage peut avoir n'importe quel type entier ; elle est comparée à la largeur du type de gauche après conversion exacte. Une valeur négative (type signé) est `< largeur` mais représente un comportement non défini : elle est **refusée** (`E-TYP-SHIFT`), voir Q-EXPR-2. | MUST |
| EXPR-3.5 | La division entière tronque vers zéro ; le reste a le signe du **dividende** (`-7 / 2 == -3`, `-7 % 2 == -1`). | MUST |
| EXPR-3.6 | `i128`/`u128` suivent les mêmes règles ; aucun mode « wrapping » n'est exposé. | MUST |
| EXPR-3.7 | Le panic est un **arrêt contrôlé** : message `panic: <msg>` sur stderr, code de sortie `1`. En mode **`run`**, le runtime installe un hook qui imprime puis appelle `exit(1)` : **aucun destructeur n'est exécuté** (l'ordre de drop est ignoré). En mode **`test`**, le runner capture le déroulement (*unwind*) et marque le test en échec sans tuer les autres tests. | MUST |
| EXPR-3.8 | Il n'existe **aucun** `catch`, aucun `catch_unwind` utilisateur. En `run`, un panic termine le processus ; en `test`, seul le runner peut l'intercepter (mécanisme interne, non exposé au langage). Le profil `pure` ne garantit pas l'absence de panic arithmétique (EFF-6.4). | MUST |

```rv
// POSITIF
fn f(a: i32, b: i32) -> i32 { return a / b; }        // panic si b == 0
fn g(a: u8) -> u8 { return a << 1; }                  // panic si >= 8
fn h(a: i32) -> i32 { return a % 3; }                 // signe du dividende

// NÉGATIF (runtime) — panic: attempt to shift left with overflow
fn i(a: u8) -> u8 { return a << 8; }
// NÉGATIF (runtime) — panic: attempt to subtract with overflow
fn j(a: u8) -> u8 { return a - 1; }                   // si a == 0
// NÉGATIF — E-TYP-SHIFT (décalage négatif)
fn k(a: i32, n: i32) -> i32 { return a << n; }         // n < 0 refusé statiquement
```

## 4. Flottants IEEE-754

| ID | Règle | Niveau |
|---|---|---|
| EXPR-4.1 | `f32`/`f64` suivent IEEE-754 binaire (arrondi au plus proche, égalité au pair) pour `+ - * / %` et `sqrt`. | MUST |
| EXPR-4.2 | Aucun piège sur les flottants : division par `0.0` produit `±inf`, `0.0/0.0` produit `NaN`, sans panic. | MUST |
| EXPR-4.3 | `NaN != NaN` est **vrai** (`!=` rend `true`) et toutes les comparaisons avec `NaN` rendent `false`, sauf `!=`. `NaN` n'est jamais égal à lui-même. | MUST |
| EXPR-4.4 | `0.0 == -0.0` est **vrai** ; `1.0/0.0 == +inf` et `1.0/-0.0 == -inf` : le signe du zéro est conservé par l'arithmétique IEEE. Sa représentation en bits n'est **pas** observable (pas de `to_bits`, voir Q-EXPR-4). | MUST |
| EXPR-4.5 | Aucun mode « fast math » : le compilateur **MUST NOT** activer `-ffast-math`, la fusion FMA, ni la réassociation. `a + b + c` est `(a + b) + c`. | MUST |
| EXPR-4.6 | Les valeurs dénormalisées sont conservées (aucun « flush to zero ») ; leur comportement suit IEEE-754. | MUST |
| EXPR-4.7 | Aucune constante `NaN`/`inf` littérale : elles s'obtiennent par calcul (`0.0/0.0`, `1.0/0.0`) ou par `math::NAN` si ce nom est retenu (voir [`stdlib.md`](stdlib.md) ; en l'état `math::NAN` n'existe pas → `E-RES-UNKNOWN`). | MUST |
| EXPR-4.8 | `f32` et `f64` ne se mélangent pas implicitement (`E-TYP-MISMATCH`) ; le passage est explicite par `as` (TYP-13.2). | MUST |

```rv
// POSITIF
fn f(x: f64) -> bool {
    let nan = 0.0 / 0.0;
    let inf = 1.0 / 0.0;
    return (nan != nan) && (x < inf) && (0.0 == -0.0);
}

// NÉGATIF — E-TYP-MISMATCH (mélange f32/f64)
fn g(a: f32, b: f64) -> f64 { return a + b; }
```

## 5. Blocs, `if`, `match` comme expressions

| ID | Règle | Niveau |
|---|---|---|
| EXPR-5.1 | Un bloc `{ ... }` est une expression ; sa valeur est sa *tail_expr* (dernière expression sans `;`), ou `()` s'il n'y en a pas. Une expression **block_like** (`{}`, `if`, `match`, `while`, `loop`, `for`) en position d'instruction n'exige pas `;` ([`grammar.md`](grammar.md) G-6.1) ; sa valeur est ignorée et doit être `()` (EXPR-5.6). | MUST |
| EXPR-5.2 | Toutes les branches d'un `if` avec `else` et tous les bras d'un `match` **MUST** avoir le même type, sans conversion (`E-TYP-MISMATCH`). | MUST |
| EXPR-5.3 | Un `if` sans `else` a le type `()` : il ne peut pas produire de valeur (`E-TYP-IF-NO-ELSE`). | MUST |
| EXPR-5.4 | L'évaluation d'un `if`/`match` n'évalue **qu'une** branche : les autres ne produisent aucun effet. | MUST |
| EXPR-5.5 | L'ordre d'examen des bras de `match` est l'ordre textuel ; le premier bras dont le pattern correspond l'emporte (les gardes n'existant pas, la correspondance est purement structurelle). | MUST |
| EXPR-5.6 | Toute **instruction-expression** — y compris une block_like sans `;` — **MUST** avoir le type `()` ; une expression de type non-`()` utilisée en instruction est `E-TYP-STATEMENT-VALUE`. | MUST |
| EXPR-5.7 | Un `match` ou un `if` de type non-`()` placé en instruction (hors *tail_expr*) est une erreur (EXPR-5.6) : le placer en *tail_expr* ou l'affecter (`let x = match ...;`). | MUST |
| EXPR-5.8 | `match` est **exhaustif** (design §3.4) : enums, `bool`, `Option`, `Result` vérifiés par le checker (`E-TYP-NONEXHAUSTIVE`), entiers exigeant `_`. Un `match` non exhaustif est refusé à la compilation, pas seulement par `rustc`. | MUST |
| EXPR-5.9 | Un bras inatteignable (après un `_` ou une couverture complète) n'est **pas** une erreur, n'est pas supprimé et ne produit **aucun diagnostic** (liste d'avertissements fermée, [`diagnostics.md`](diagnostics.md) §2). | MUST |
| EXPR-5.10 | Un pattern de `match` qui déplace une valeur mobile déplace effectivement la valeur du scrutinee ; utiliser `&` n'existe pas en pattern (G-6.38), donc un `match` sur une valeur mobile **consomme** le scrutinee. `match v { ... }` puis usage de `v` → `E-OWN-USE-AFTER-MOVE`. | MUST |
| EXPR-5.11 | `while`, `loop` et `for` sont des **expressions de contrôle** dans l'AST (`While`, `Loop`, `For`) et ont le type `()` : elles ne produisent pas de valeur et ne peuvent pas être la *tail_expr* d'un bloc de type non-`()`. | MUST |

```rv
// POSITIF
fn signe(x: i32) -> i32 {
    let s = if x < 0 { -1 } else if x == 0 { 0 } else { 1 };
    return match s {
        -1 => -1,
        0 => 0,
        _ => 1,
    };
}

// NÉGATIF — E-TYP-IF-NO-ELSE
fn g(c: bool) -> i32 { let x = if c { 1 }; return x; }
// NÉGATIF — E-TYP-STATEMENT-VALUE
fn h(c: bool) { if c { 1 } else { 2 }; }
// NÉGATIF — E-OWN-USE-AFTER-MOVE (match consomme un mobile)
fn i(s: String) -> usize { match s { _ => { } } return s.len(); }
```

## 6. Boucles et contrôle

| ID | Règle | Niveau |
|---|---|---|
| EXPR-6.1 | `while cond { body }` : `cond` est réévaluée **avant chaque** itération ; le corps est un bloc de type `()`. | MUST |
| EXPR-6.2 | `loop { body }` boucle jusqu'à `break;`. Un `loop` sans `break` atteignable **diverge** (il ne termine pas) ; ce n'est pas une erreur statique. | MUST |
| EXPR-6.3 | `for x in a..b { body }` : `a` et `b` sont évalués **une seule fois**, dans cet ordre, avant la première itération ; `x` prend `a, a+1, ..., b-1` et si `a >= b` le corps n'est jamais exécuté. `for x in v { body }` où `v: Vec<T>` évalue `v` une fois puis le **consomme** ; `x` prend chaque `T` possédé dans l'ordre des indices. Dans les deux formes, `x` est une **nouvelle** liaison immuable. | MUST |
| EXPR-6.4 | Forme plage : `a` et `b` doivent être du **même** type entier (`E-TYP-MISMATCH`) et `x` a ce type. Forme collection : `v` doit être un `Vec<T>` possédé (`E-TYP-MISMATCH` pour `&Vec<T>` ou `&[T]`) et `x: T`. | MUST |
| EXPR-6.5 | `break;` sort de la boucle **la plus interne** ; `continue;` passe à l'itération suivante. Aucune étiquette (`'outer`) n'existe (`E-UNSUPPORTED-TOKEN`). | MUST |
| EXPR-6.6 | `break`/`continue` sont interdits hors boucle (`E-TYP-BREAK-OUTSIDE`). Un `break` dans une **fonction imbriquée** ne peut pas sortir d'une boucle de la fonction externe : les fonctions ne capturent rien. | MUST |
| EXPR-6.7 | `while`/`loop`/`for` ont le type `()` ; ils ne produisent pas de valeur et ne sont pas des expressions de valeur. | MUST |
| EXPR-6.8 | Aucune garantie de terminaison générale n'est donnée (design, absence de `total`). Un programme peut ne pas terminer. | MUST |
| EXPR-6.9 | Forme plage : `for i in a..b` ne modifie rien et la borne est figée à l'évaluation initiale (modifier un `Vec` pendant `for i in 0..v.len()` est autorisé). Forme collection : `for x in v` **déplace** `v` (OWN-5.10), donc toute modification ou réutilisation de `v` pendant/après la boucle est `E-OWN-USE-AFTER-MOVE`. | MUST |
| EXPR-6.10 | La récursion est autorisée, sans garantie de profondeur : un débordement de pile n'est **pas** spécifié (il peut produire un arrêt du processus, voir [`concurrency.md`](concurrency.md) §7 et Q-EXPR-5). | MUST |

```rv
// POSITIF
fn somme(n: i32) -> i32 {
    let mut s = 0;
    for i in 0..n { s += i; }
    let mut k = n;
    while k > 0 { k += -1; if k == 3 { continue; } if k == 1 { break; } }
    loop { break; }
    return s;
}

// NÉGATIF — E-TYP-MISMATCH (bornes de types différents)
fn g(a: i32, b: i64) { for i in a..b { } }
// NÉGATIF — E-TYP-BREAK-OUTSIDE
fn h() { break; }
// NÉGATIF — E-UNSUPPORTED-TOKEN
fn i() { 'outer: loop { break; } }
```

## 7. Appels, méthodes, champs, indexation

| ID | Règle | Niveau |
|---|---|---|
| EXPR-7.1 | L'arité de l'appel doit correspondre exactement à la signature, arguments dans l'ordre, sans argument nommé ni valeur par défaut (`E-TYP-ARITY`). | MUST |
| EXPR-7.2 | Les arguments sont passés **par valeur** : un argument mobile est déplacé, un `Copy` est copié. Le passage par référence est explicite (`&x`, `&mut x`). | MUST |
| EXPR-7.3 | Un argument `&mut T` exige une place **mutable** et **exclusivement** empruntable pendant l'instruction (OWN-6). | MUST |
| EXPR-7.4 | Une méthode est résolue sur le type du receveur, par nom, dans l'`impl` du type nominal. `&self`/`&mut self`/`self` déterminent le mode de passage ; un receveur `self` déplace la valeur. | MUST |
| EXPR-7.5 | Les champs sont accessibles par `.nom` ; un champ privé hors de son module est `E-RES-PRIVATE`. Un champ inexistant est `E-TYP-FIELD`. | MUST |
| EXPR-7.6 | L'indexation `a[i]` est définie sur `[T; N]`, `Vec<T>` et `&[T]`, avec `i: usize` ; hors bornes → **panic** (`panic: index out of bounds: the len is N but the index is i`). Aucune indexation de `String` (`E-TYP-INDEX`). | MUST |
| EXPR-7.7 | `Vec<T>` et `[T; N]` partagent la syntaxe d'indexation mais **pas** la sémantique de déplacement : voir TYP-9.3 (index de `Vec` non-`Copy` = `E-OWN-PARTIAL-MOVE`). | MUST |
| EXPR-7.8 | Un appel de fonction associée `Type::f(...)` ne prend pas de `self` ; une méthode par point ne prend pas de `Type::` explicite. Un `Type::methode(receveur, ...)` explicite (« UFCS ») n'existe pas (`E-RES-UNKNOWN`). | MUST |
| EXPR-7.9 | L'appel d'une valeur non-fonction (`x(1)`) est `E-TYP-CALL`. Il n'existe pas de pointeur de fonction ni de closure. | MUST |
| EXPR-7.10 | Les arguments d'un appel sont évalués avant l'appel ; l'appel lui-même est une instruction complète : tous les emprunts créés par les arguments **expirent à la fin de l'appel** (OWN-5). | MUST |

```rv
// POSITIF
impl Point {
    fn new(x: f64, y: f64) -> Point { return Point { x: x, y: y }; }
    fn norme(&self) -> f64 { return math::sqrt(self.x * self.x + self.y * self.y); }
    fn decale(&mut self, dx: f64) { self.x += dx; }
}
fn f() -> f64 {
    let mut p = Point::new(3.0, 4.0);
    p.decale(1.0);
    let n = p.norme();
    let a = [1, 2, 3];
    return n + a[0] as f64;
}

// NÉGATIF — E-TYP-ARITY
fn g() { let p = Point::new(1.0); }
// NÉGATIF — E-TYP-CALL
fn h(x: i32) { x(1); }
// NÉGATIF — E-RES-PRIVATE (champ privé hors module)
fn i(p: Point) -> f64 { return p.y; }
```

## 8. `?` — propagation d'erreur

| ID | Règle | Niveau |
|---|---|---|
| EXPR-8.1 | `e?` où `e: Result<T, E>` est valide seulement si la fonction englobante retourne `Result<U, E>` avec **exactement le même** type d'erreur `E`. La valeur de succès est `T`. | MUST |
| EXPR-8.2 | `e?` où `e: Option<T>` est valide seulement si la fonction englobante retourne `Option<U>`. | MUST |
| EXPR-8.3 | Toute autre combinaison est `E-TYP-TRY` (design §3.8). En particulier `?` sur `Result<T, Error>` dans une fonction retournant `()` est refusé. | MUST |
| EXPR-8.4 | `e?` sur `Err(err)` **retourne immédiatement** `Result::Err(err)` de la fonction englobante, en déplaçant `err`. Aucune conversion d'erreur, aucun `From` (pas de traits). | MUST |
| EXPR-8.5 | `e?` sur `Option::None` retourne immédiatement `Option::None`. | MUST |
| EXPR-8.6 | `?` est autorisé dans `main` lorsque `main` a la signature `fn main() -> Result<(), Error>` ; l'erreur non gérée est alors imprimée et le code de sortie est `1`. | MUST |
| EXPR-8.7 | `?` n'est pas utilisable dans une fonction `-> ()`, ni dans un bloc `const`, ni dans un `impl` de constante. | MUST |
| EXPR-8.8 | `?` ne s'applique qu'à une **valeur** `Result`/`Option`, pas à une place empruntée (`&e?` n'existe pas). | MUST |

```rv
// POSITIF
fn lit(p: &str) -> Result<String, Error> {
    let s = fs::read_to_string(p)?;       // Err(Error) remonte tel quel
    return Result::Ok(s);
}

fn premier(v: Vec<i32>) -> Option<i32> { return Option::Some(v[0]); }
fn deux(v: Vec<i32>) -> Option<i32> { let a = premier(v)?; return Option::Some(a); }

fn main() -> Result<(), Error> {
    let s = lit("a.txt")?;
    io::println(s.as_str());
    return Result::Ok(());
}

// NÉGATIF — E-TYP-TRY (fonction -> ())
fn g(p: &str) { let s = fs::read_to_string(p)?; }
// NÉGATIF — E-TYP-TRY (type d'erreur différent)
fn h(p: &str) -> Result<(), String> { let s = fs::read_to_string(p)?; return Result::Ok(()); }
```

## 9. Assignation et mutation

| ID | Règle | Niveau |
|---|---|---|
| EXPR-9.1 | `place = e;` exige une place **mutable**, un type identique (aucune conversion, `E-TYP-MISMATCH`), et évalue `e` **avant** d'écrire la place. | MUST |
| EXPR-9.2 | L'assignation **remplace** la valeur : l'ancienne valeur est détruite selon l'ordre de drop, **immédiatement** (pas à la fin du bloc), si elle est mobile et non `Copy`. | MUST |
| EXPR-9.3 | Assigner à une place **non initialisée** l'initialise (OWN-4.4) ; assigner à une place **déplacée** la **ré-initialise** (ce n'est pas une utilisation : voir OWN-4.4). Lire une place déplacée reste `E-OWN-USE-AFTER-MOVE`. | MUST |
| EXPR-9.4 | La **famille complète** des opérateurs composés est acceptée : `+= -= *= /= %= &= |= ^= <<= >>=`. Chacun suit `place = place op e` avec **une seule** évaluation de `place` (pas de double évaluation d'un index ou d'un champ). | MUST |
| EXPR-9.5 | Une forme non reconnue (`**=`, `//=`, `&&=`) est `E-PARSE-UNEXPECTED`. La famille ci-dessus est fermée : aucune autre n'est acceptée. | MUST |
| EXPR-9.6 | L'assignation d'un agrégat entier (`p = Point { .. }`) est valide si le type correspond ; l'assignation **partielle** d'un struct (`p.x = 1.0`) est valide si `p` est mutable et si `p` n'est pas emprunté. | MUST |
| EXPR-9.7 | Une assignation ne peut pas cibler un *place* emprunté par un emprunt actif : conflit `E-OWN-BORROW-CONFLICT` (OWN-6). | MUST |
| EXPR-9.8 | L'assignation est une instruction, de type `()` ; elle ne produit pas de valeur et ne peut pas figurer en position d'expression (G-6.8). | MUST |

```rv
// POSITIF
fn f(mut p: Point, mut v: Vec<i32>) {
    p.x = 1.0;
    p = Point { x: 2.0, y: 3.0 };     // remplace, détruit l'ancien p immédiatement
    v.push(1);
    v[0] = 2;
    let mut n: i32;
    n = 5;                            // initialise
    n += 1;
}

// NÉGATIF — E-OWN-IMMUTABLE
fn g(p: Point) { p.x = 1.0; }
// NÉGATIF — E-OWN-USE-AFTER-MOVE (lire une place déplacée)
fn h() { let s = String::from("a"); let t = s; let n = s.len(); }
// POSITIF — la ré-initialisation d'une place déplacée est permise (OWN-4.4)
fn h2() { let mut s = String::from("a"); let t = s; s = String::from("b"); }
// NÉGATIF — E-TYP-MISMATCH
fn i(mut n: i32) { n = "texte"; }
// NÉGATIF — E-PARSE-EXPECTED
fn j(mut n: i32) { n *= 2; }
```

## 10. Conversions explicites (`as`)

| ID | Règle | Niveau |
|---|---|---|
| EXPR-10.1 | `as` est évalué après son opérande et produit immédiatement la valeur convertie ; il n'a aucun effet de bord. | MUST |
| EXPR-10.2 | La sémantique exacte (saturant, troncature, arrondi) est celle de [`types.md`](types.md) TYP-13.2 ; elle est **totale** : aucun `as` admis ne panic ni ne produit d'UB. | MUST |
| EXPR-10.3 | `as` ne change pas l'ordre d'évaluation ni la nature place/valeur de son opérande : `(x as u8)` est une valeur, non une place. | MUST |
| EXPR-10.4 | `f64 as i32` sur `NaN` rend `0`, sur `+inf` rend `i32::MAX`, sur `-inf` rend `i32::MIN` ; ces valeurs sont **spécifiées**, pas des « comportements de plateforme ». | MUST |
| EXPR-10.5 | Aucun `as` implicite n'est inséré par le compilateur pour « réparer » un mismatch : `E-TYP-MISMATCH` est définitif. | MUST |

```rv
// POSITIF
fn f(x: f64) -> u8 { return x as u8; }            // 300.0 -> 255, -1.0 -> 0, NaN -> 0
fn g(c: char) -> u32 { return c as u32; }

// NÉGATIF — E-TYP-CAST
fn h(x: f64) -> char { return x as char; }
```

## 11. Panic et divergence

| ID | Règle | Niveau |
|---|---|---|
| EXPR-11.1 | `panic(msg)` est un **intrinsèque** du prélude ([`stdlib.md`](stdlib.md) §10) : il imprime `panic: <msg>` sur stderr et termine avec le code `1`. Son type est divergent (compatible avec tout type attendu). | MUST |
| EXPR-11.2 | Les intrinsèques `assert`, `assert_eq`, `panic` sont les seules sources de panic **explicites** du langage ; les panic **implicites** sont : débordement, division/reste par zéro, décalage hors largeur, index hors bornes, `unwrap` sur `None`/`Err`, `process::exit` (sortie, pas panic). | MUST |
| EXPR-11.3 | Une expression divergente (`return`, `break`, `continue`, `panic`, appel de fonction divergente) est compatible avec n'importe quel type attendu (`!` interne ; sa forme **nommable** est limitée par G-5.11 et TYP-1.6, qui priment sur l'ancienne formulation « non nommable »). | MUST |
| EXPR-11.4 | Un panic déclenché pendant l'évaluation d'un agrégat laisse l'agrégat non construit et n'écrit rien de partiel dans une place existante. | MUST |
| EXPR-11.5 | Aucun `Result` n'est produit par un panic : un panic n'est **pas** rattrapable. | MUST |

```rv
// POSITIF
fn f(o: Option<i32>) -> i32 {
    match o {
        Option::Some(x) => x,
        Option::None => panic("option vide"),     // divergent
    }
}

// NÉGATIF — E-TYP-ARITY (assert_eq exige 2 arguments)
fn g() { assert_eq(1); }
```

## 12. Résumé des codes émis par ce document

| Code | Contexte |
|---|---|
| `E-TYP-MISMATCH` | types d'opérandes/branches/arguments incompatibles |
| `E-TYP-ARITY` | mauvais nombre d'arguments/paramètres génériques |
| `E-TYP-IF-NO-ELSE` | `if` sans `else` utilisé en valeur |
| `E-TYP-STATEMENT-VALUE` | expression non-`()` en position d'instruction |
| `E-TYP-NONEXHAUSTIVE` | `match` non exhaustif |
| `E-TYP-FIELD` | champ inexistant |
| `E-TYP-CALL` | appel d'une non-fonction |
| `E-TYP-TRY` | `?` hors contexte compatible |
| `E-TYP-INDEX` | indexation invalide (type ou type non indexable) |
| `E-TYP-CAST` | `as` hors paires admises |
| `E-TYP-SHIFT` | décalage négatif |
| `E-TYP-LITERAL-RANGE` | littéral hors du type inféré |
| `E-TYP-AMBIGUOUS` | inférence non résolue |
| `E-OWN-IMMUTABLE` | affectation d'une place immuable |
| `E-OWN-BORROW-CONFLICT` | assignation sous emprunt actif |
| `E-OWN-USE-AFTER-MOVE` | lecture/écriture d'une place déplacée |
| `E-OWN-PARTIAL-MOVE` | déplacement hors d'un `Vec` |
| `E-UNSUPPORTED-TOKEN` | étiquette de boucle |
| `E-RES-UNKNOWN` | appel `Type::methode(recv, ...)` |

## 13. Questions ouvertes

- **Q-EXPR-1 (résolue par le Lead)** — En mode `run`, panique = **arrêt sans destructeurs** (hook qui imprime puis `exit(1)`) ; en mode `test`, le runner capture l'*unwind* et marque le test en échec. Les destructeurs ne s'exécutent donc jamais sur un panic, et l'ordre de drop n'est pas observable (OWN-9.6).
- **Q-EXPR-2** — EXPR-3.4 refuse statiquement un décalage négatif. Une alternative est un panic runtime. Le refus statique n'est possible que si la valeur est connue ; pour une valeur dynamique, la règle est un panic runtime (`panic: negative shift`). Clarifier la formulation finale.
- **Q-EXPR-3** — Un `loop` sans `break` diverge ; aucun avertissement n'existe (liste fermée). Un futur lint exigerait un code `W-*` ratifié.
- **Q-EXPR-4** — Sans `f64::to_bits`, l'identité bit-à-bit des flottants n'est **pas** observable ; l'oracle de conformance ne peut vérifier que des prédicats IEEE (`==`, `<`), pas la représentation de `NaN`. Si une vérification stricte est nécessaire, ajouter `math::to_bits(x: f64) -> u64` / `from_bits` — extension non prévue par le design.
- **Q-EXPR-5** — Le débordement de pile (récursion profonde) n'est pas spécifié : le processus peut être tué par l'OS (code de sortie non déterministe). Le design ne le mentionne pas. Recommandation : documenter comme « non spécifié, ne pas s'y fier » et ne pas le tester en conformance.
- **Q-EXPR-6** — EXPR-9.2 détruit l'ancienne valeur immédiatement à l'assignation. Cela rend l'ordre des destructeurs observable (via les effets d'un `drop`… mais `Drop` utilisateur est interdit, donc **aucun effet observable** en Core 1.0). La règle est donc non observable aujourd'hui ; elle doit être fixée maintenant pour ne pas contraindre une future extension.
- **Q-EXPR-7** — `for` figé sur `0..n` (G-6.24) : itérer un `Vec` par valeur est impossible. Faut-il `for i in 0..v.len()` uniquement (statu quo) ? C'est cohérent avec l'absence d'itérateurs, mais à confirmer pour l'ergonomie DSP (buffer par blocs).
