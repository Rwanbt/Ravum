# Ravum Core 1.0 — Grammaire syntaxique (EBNF)

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.1–§3.4.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR**. Aucune implémentation n'est revendiquée (`NOT_IMPLEMENTED`).
> Les tokens sont ceux de [`lexical.md`](lexical.md) ; les types et règles de typage sont dans [`types.md`](types.md) ; les codes dans [`diagnostics.md`](diagnostics.md).

## 1. Conventions et objectif

- La grammaire ci-dessous est **complète** : tout programme `.rv` valide est dérivable de `program`, et toute dérivation produit un AST unique.
- Elle est **LL(1) exploitable** par un parser descendant récursif à un token de lookahead, sauf mention explicite « lookahead 2 » (§6.5 ``pour les expressions de bloc, §6.4 `struct` littéral vs bloc).
- `?` en suffixe d'un non-terminal = optionnel ; `{ X }` = répétition ≥ 0 ; `[ X ]` = optionnel ; `"x"` = terminal.
- **Aucune règle d'ambiguïté n'est laissée à l'implémentation** : les cas litigieux sont tranchés ici et signalés par un identifiant `G-n.m`.
- Les erreurs de syntaxe portent un code `E-PARSE-*` ; les constructions syntaxiquement valides mais hors Core 1.0 portent un code `E-UNSUPPORTED-*` (§ [`unsupported.md`](unsupported.md)).

## 2. Structure du programme

```ebnf
program        = { item } EOF ;
item           = fn_item
               | test_item
               | struct_item
               | enum_item
               | impl_item
               | const_item
               | mod_item
               | use_item ;

visibility     = [ "pub" ] ;
```

| ID | Règle | Niveau |
|---|---|---|
| G-2.1 | Un fichier contient zéro ou plusieurs items, dans n'importe quel ordre. L'ordre de déclaration n'a **aucun** effet sémantique (pas d'ordre d'initialisation). | MUST |
| G-2.2 | Tout `item` commence par un token de tête non ambigu : `pub`, `fn`, `test`, `struct`, `enum`, `impl`, `const`, `mod`, `use`. Tout autre token en position d'item produit `E-PARSE-EXPECTED`. | MUST |
| G-2.3 | Il n'y a **pas** de `;` après un item. Un `;` isolé en position d'item produit `E-PARSE-EXPECTED`. | MUST |
| G-2.4 | `pub` n'est valide que sur `fn`, `struct`, `enum`, `const`, `mod`, `use` et sur les champs/variantes (§4.2, §4.3). `pub impl` est refusé (`E-PARSE-EXPECTED`). | MUST |
| G-2.5 | Les attributs `#[...]` produisent `E-UNSUPPORTED-ATTR` en toute position ; ils ne sont jamais ignorés. | MUST |
| G-2.6 | Un item imbriqué dans un `fn` (function item) est refusé (`E-PARSE-EXPECTED`). Seuls les blocs d'instructions y sont autorisés. | MUST |

## 3. Fonctions, effets, génériques

```ebnf
fn_item        = visibility "fn" IDENT [ generic_params ] "(" [ params ] ")" [ "->" type ] [ effects_clause ] block ;
params         = param { "," param } [ "," ] ;
param          = IDENT ":" type ;
effects_clause = "effects" "(" [ effect_list ] ")" ;
effect_list    = effect { "," effect } [ "," ] ;
effect         = IDENT ;                      (* validé contre les 9 groupes connus *)

generic_params = "<" IDENT { "," IDENT } [ "," ] ">" ;

test_item      = "test" "fn" IDENT "(" ")" [ "->" type ] block ;
impl_member    = visibility "fn" IDENT [ generic_params ] "(" [ self_param { "," param } [ "," ] ] ")"
                 [ "->" type ] [ effects_clause ] block ;
self_param     = "&" [ "mut" ] "self" | "self" ;
```

| ID | Règle | Niveau |
|---|---|---|
| G-3.1 | Le type de retour est obligatoire dès qu'une valeur est renvoyée ; omis, il vaut `()`. `-> ()` explicite est accepté. | MUST |
| G-3.2 | `effects(...)` vient **après** le type de retour (ou après `)` si le retour est omis) et **avant** le bloc. Toute autre position est `E-PARSE-UNEXPECTED`. | MUST |
| G-3.3 | Un effet inconnu (pas un des 9 groupes) est `E-EFF-UNKNOWN`, pas une erreur de parse. | MUST |
| G-3.4 | Un paramètre est **toujours** `IDENT ":" type`. Le pattern de paramètre (`(a, b): (i32, i32)`) n'existe pas en Core 1.0 (`E-PARSE-EXPECTED`). | MUST |
| G-3.5 | `generic_params` déclare uniquement des paramètres de type nus ; aucune borne, aucun `where`, aucune lifetime. `where` produit `E-UNSUPPORTED-WHERE` ; `T: Trait` produit `E-UNSUPPORTED-TRAIT`. | MUST |
| G-3.6 | `test_item` **MUST NOT** être `pub` et n'a **aucun paramètre**. Son type de retour est `()` ou `Result<(), Error>` (`E-TYP-MAIN` sinon) ; un test qui rend `Err` est un **échec de test** (code de sortie `1`). | MUST |
| G-3.7 | `self_param` est obligatoire en premier paramètre d'une méthode d'`impl` qui porte `self` ; une fonction associée n'a pas de `self`. Un `self` hors `impl` produit `E-RES-SELF`. | MUST |
| G-3.8 | Aucune valeur de retour « nulle part » : `->` sans type est `E-PARSE-EXPECTED`. | MUST |
| G-3.9 | Le corps d'un `fn` est **toujours** un bloc `{ ... }`. `fn f();` (déclaration sans corps) est `E-PARSE-EXPECTED`. | MUST |
| G-3.10 | `unsafe fn`, `extern fn`, `async fn` sont reconnus puis refusés : `E-UNSUPPORTED-UNSAFE`, `E-UNSUPPORTED-EXTERN`, `E-UNSUPPORTED-ASYNC`. | MUST |

```rv
// POSITIF
pub fn somme<T>(a: T, b: T) -> T effects() { return a; }   // effet déclaré vide
fn lire(p: &str) -> Result<String, Error> effects(fs, alloc, io) { return fs::read_to_string(p); }

// NÉGATIF — E-PARSE-UNEXPECTED (effects avant le type de retour)
fn f() effects(io) -> i32 { return 0; }

// NÉGATIF — E-UNSUPPORTED-WHERE
fn f<T>(x: T) -> T where T: Copy { return x; }

// NÉGATIF — E-PARSE-EXPECTED
test fn mauvais(x: i32) { }
```

## 4. Types nominaux : struct, enum, impl, const

```ebnf
struct_item    = visibility "struct" IDENT [ generic_params ] "{" [ field_list ] "}" ;
field_list     = field { "," field } [ "," ] ;
field          = visibility IDENT ":" type ;

enum_item      = visibility "enum" IDENT [ generic_params ] "{" [ variant_list ] "}" ;
variant_list   = variant { "," variant } [ "," ] ;
variant        = visibility IDENT [ tuple_fields | record_fields ] ;
tuple_fields   = "(" type { "," type } [ "," ] ")" ;
record_fields  = "{" [ field_list ] "}" ;

impl_item      = "impl" [ generic_params ] impl_target "{" { impl_member } "}" ;
impl_target    = type_path ;                  (* type nominal local, types génériques nus *)

const_item     = visibility "const" IDENT ":" type "=" const_expr ";" ;

mod_item       = visibility "mod" IDENT ( ";" | "{" { item } "}" ) ;
use_item       = visibility "use" use_path ";" ;
```

| ID | Règle | Niveau |
|---|---|---|
| G-4.1 | `struct` vide (`struct U { }`) et `struct U {}` sont valides : type sans champ, toujours `Copy`. | MUST |
| G-4.2 | Un champ peut être `pub` ; par défaut privé. `pub` sur un champ d'enum est accepté syntaxiquement et **sans effet** (la visibilité d'une variante est celle de l'enum) — voir Q-GRAM-2. | MUST |
| G-4.3 | Une variante est soit unité (`Empty`), soit tuple (`Circle(f64)`), soit record (`Rect { w: f64, h: f64 }`). Les trois formes ne se mélangent pas dans une variante. | MUST |
| G-4.4 | Un littéral de struct exige `champ: expr` pour chaque champ fourni ; le raccourci `champ` (shorthand) n'existe **pas** en expression (`E-PARSE-EXPECTED`), alors qu'il est autorisé en **pattern** (§6.6, design §3.4). | MUST |
| G-4.5 | `impl_target` **MUST** être un chemin de type nominal défini dans le package courant ; `impl i32`, `impl String`, `impl Vec<i32>` produisent `E-RES-IMPL-SELF-TYPE`. | MUST |
| G-4.6 | Les paramètres génériques d'un `impl<T> Foo<T>` doivent correspondre exactement aux paramètres du type cible, dans l'ordre. Toute autre forme est `E-RES-IMPL-UNKNOWN`. | MUST |
| G-4.7 | Un `impl` ne peut pas contenir de `const`, de `struct`, ni de `impl` imbriqué : `E-PARSE-EXPECTED`. | MUST |
| G-4.8 | `const` exige un type explicite, un `=` et un `;`. `const N = 10;` est `E-PARSE-EXPECTED`. | MUST |
| G-4.9 | `const_expr` est le sous-ensemble **fermé** de §5.5 : littéraux, opérateurs arithmétiques/logiques sur scalaires, `as`, parenthèses. Aucun appel, aucune allocation, aucune référence. Toute autre expression est `E-TYP-CONST`. | MUST |
| G-4.10 | `mod foo;` (fichier) et `mod foo { ... }` (inline) sont exclusifs ; `mod foo { } ;` est `E-PARSE-EXPECTED` (le `;` est refusé après un module inline). | MUST |
| G-4.11 | `use` n'accepte ni `*`, ni `as`, ni `pub use` : `E-UNSUPPORTED-TOKEN`, `E-UNSUPPORTED-TOKEN`, `E-UNSUPPORTED-TOKEN`. | MUST |
| G-4.12 | `impl` sans aucun membre est valide (`impl P { }`), sans effet. | MUST |
| G-4.13 | Un `struct`/`enum` récursif sans indirection (aucun `Box` en Core 1.0) est `E-TYP-RECURSIVE` (voir [`types.md`](types.md) TYP-12). | MUST |

```rv
// POSITIF
pub struct Point { pub x: f64, y: f64 }
enum Shape { Circle(f64), Rect { w: f64, h: f64 }, Empty }
impl Point {
    fn norme(&self) -> f64 { return math::sqrt(self.x * self.x + self.y * self.y); }
    fn new(x: f64) -> Point { return Point { x: x, y: 0.0 }; }   // pas de shorthand
}
const N: i32 = 10;

// NÉGATIF — E-PARSE-EXPECTED
fn f(x: f64) -> Point { return Point { x, y: 0.0 }; }

// NÉGATIF — E-RES-IMPL-SELF-TYPE
impl i32 { fn double(&self) -> i32 { return *self; } }

// NÉGATIF — E-PARSE-EXPECTED (type manquant)
const M = 1;
```

## 5. Types

```ebnf
type           = primitive_type
               | never_type
               | text_ref_type
               | ref_type
               | tuple_type
               | array_type
               | path_type ;

primitive_type = "i8" | "i16" | "i32" | "i64" | "i128"
               | "u8" | "u16" | "u32" | "u64" | "u128"
               | "isize" | "usize" | "f32" | "f64" | "bool" | "char" | "()" ;
never_type     = "!" ;                      (* type Never, voir G-5.11 *)
text_ref_type  = "&" "str" ;
ref_type       = "&" [ "mut" ] ( type | text_ref_type_inner ) ;
tuple_type     = "(" [ type { "," type } [ "," ] ] ")" ;   (* "()" est primitive_type *)
array_type     = "[" type ";" INT_LIT "]" ;
path_type      = type_path ;
type_path      = type_segment { "::" type_segment } ;
type_segment   = IDENT | "Self" | "crate" | "super" | "self" [ generic_args ] ;
generic_args   = "<" type { "," type } [ "," ] ">" ;
```

| ID | Règle | Niveau |
|---|---|---|
| G-5.1 | `()` est un type primitif unique (type unité), pas un tuple vide syntaxique distinct. | MUST |
| G-5.2 | Un tuple à un élément **MUST** avoir une virgule finale : `(i32,)`. `(i32)` est une parenthèse de groupement et désigne `i32`. | MUST |
| G-5.3 | `&str` est le seul usage valide du token `str` en position de type. `str` seul, `str` en paramètre générique ou en champ est `E-RES-UNKNOWN`. | MUST |
| G-5.4 | `&mut str` n'existe pas : `E-RES-UNKNOWN`. | MUST |
| G-5.5 | La longueur d'un tableau est un littéral entier **décimal sans suffixe**, de valeur > 0 ; `[T; 0]` est `E-TYP-ARRAY-REPEAT` (tableaux vides interdits, voir Q-TYP-4). `[T; N]` avec `N` un identifiant est `E-TYP-ARRAY-REPEAT`. | MUST |
| G-5.6 | Il n'existe **aucune** lifetime explicite (`&'a T`, `'static`) : `E-UNSUPPORTED-TOKEN`. | MUST |
| G-5.7 | Il n'existe **aucun** pointeur brut (`*const T`, `*mut T`) : `E-UNSUPPORTED-UNSAFE`. | MUST |
| G-5.8 | Il n'existe **aucun** `impl Trait`, `Box<T>`, ni `str` autonome : `E-UNSUPPORTED-IMPL-TRAIT`, `E-RES-UNKNOWN`. `dyn` est un **identifiant valide** : `dyn T` est un nom de type inconnu (`E-RES-UNKNOWN`). | MUST |
| G-5.9 | Les arguments génériques ne sont admis **qu'en position de type**. Le turbofish `f::<i32>(x)` n'existe pas : `E-PARSE-UNEXPECTED`. | MUST |
| G-5.10 | `Self` est un alias du type cible de l'`impl` englobant ; hors `impl`, `E-RES-SELF`. | MUST |
| G-5.11 | `!` est le **type Never** : aucune valeur, aucune taille. Il est le type des expressions divergentes (`return`, `break`, `continue`, `panic`, `process::exit`) et **coerce implicitement vers tout type**. Il n'est ni constructible, ni un type de paramètre, de champ, de `Vec` ou de retour d'une fonction non divergente. | MUST |

```rv
// POSITIF
fn f(a: i32, b: &str, c: &mut Vec<String>, d: (i32, bool), e: [u8; 4]) -> () { }

// NÉGATIF — E-RES-UNKNOWN (`str` seul)
fn g(s: str) { }
// NÉGATIF — E-TYP-ARRAY-REPEAT (taille nulle)
fn h(a: [u8; 0]) { }
// NÉGATIF — E-UNSUPPORTED-TOKEN
fn i(a: &'static str) { }
// NÉGATIF — E-PARSE-UNEXPECTED
fn j() { let v: Vec<i32> = Vec::<i32>::new(); }
```

### 5.5 `const_expr` (sous-ensemble fermé)

```ebnf
const_expr     = const_or ;
const_or       = const_and { "||" const_and } ;
const_and      = const_cmp { "&&" const_cmp } ;
const_cmp      = const_bitor [ cmp_op const_bitor ] ;
const_bitor    = const_bitxor { "|" const_bitxor } ;
const_bitxor   = const_bitand { "^" const_bitand } ;
const_bitand   = const_shift { "&" const_shift } ;
const_shift    = const_add { ( "<<" | ">>" ) const_add } ;
const_add      = const_mul { ( "+" | "-" ) const_mul } ;
const_mul      = const_cast { ( "*" | "/" | "%" ) const_cast } ;
const_cast     = const_unary { "as" primitive_type } ;
const_unary    = ( "-" | "!" ) const_unary | const_atom ;
const_atom     = INT_LIT | FLOAT_LIT | "true" | "false" | CHAR_LIT | STRING_LIT
               | "(" const_expr ")" ;
```

| ID | Règle | Niveau |
|---|---|---|
| G-5.11 | Un `const_expr` n'a ni variable, ni appel, ni accès de champ, ni indexation, ni `?`, ni bloc. Présence → `E-TYP-CONST`. | MUST |
| G-5.12 | L'évaluation est faite par le compilateur, en arithmétique **vérifiée** : un débordement ou une division par zéro dans un `const_expr` est `E-TYP-CONST` (erreur de compilation, jamais une valeur enveloppée silencieuse). | MUST |
| G-5.13 | Un `const` ne peut pas référencer un autre `const` en Core 1.0 (pas de dépendances entre constantes) : `E-TYP-CONST`. Voir Q-GRAM-4. | MUST |

## 6. Instructions, expressions, patterns

### 6.1 Blocs et instructions

```ebnf
block          = "{" { stmt } [ tail_expr ] "}" ;
tail_expr      = expr ;                       (* dernière expression du bloc, sans ";" *)

stmt           = let_stmt
               | assign_stmt
               | return_stmt
               | break_stmt
               | continue_stmt
               | block_like_expr [ ";" ]      (* instruction : valeur ignorée, type () exigé *)
               | expr ";"                     (* expression-statement, type () exigé *)
               | ";" ;                       (* instruction vide *)

block_like_expr = block | if_expr | match_expr | while_expr | loop_expr | for_expr ;

let_stmt       = "let" [ "mut" ] IDENT [ ":" type ] [ "=" expr ] ";" ;
assign_stmt    = place "=" expr ";"
               | place compound_op expr ";" ;
compound_op    = "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>=" ;
return_stmt    = "return" [ expr ] ";" ;
break_stmt     = "break" ";" ;
continue_stmt  = "continue" ";" ;

place          = "*" place
               | place_base { place_suffix } ;
place_base     = IDENT ;
place_suffix   = "." ( IDENT | INT_LIT )
               | "[" expr "]" ;
```

| ID | Règle | Niveau |
|---|---|---|
| G-6.1 | **Règle d'arrêt des blocs (déterministe, lookahead 1)** : en position d'instruction, le parser lit une expression. Si elle est **block_like** (`{}`, `if`, `match`, `while`, `loop`, `for`) et que le token suivant n'est ni `;` ni `}`, elle forme une **instruction sans `;`** (valeur ignorée, type `()` exigé par G-6.5). Si le token suivant est `}`, c'est la *tail_expr* du bloc. Si c'est `;`, c'est une instruction et le `;` est consommé. Une expression **non** block_like exige `;`, sauf en *tail_expr*. Toute autre configuration est `E-PARSE-EXPECTED`. | MUST |
| G-6.2 | `let` lie **un seul identifiant** ; le pattern de `let` n'existe pas (`let (a, b) = t;` → `E-PARSE-EXPECTED`). La déstructuration passe par `match`. | MUST |
| G-6.3 | `let x;` sans type ni initialiseur est `E-PARSE-EXPECTED`. `let x: T;` (initialisation différée) est valide et soumis à TYP/OWN (§ [`ownership.md`](ownership.md) OWN-1). | MUST |
| G-6.4 | `let mut x;` est valide. `let x = 1;` puis `x = 2;` est `E-OWN-IMMUTABLE`. | MUST |
| G-6.5 | Une **instruction-expression** (block_like sans `;` incluse) **MUST** avoir le type `()` ; une expression de type non-`()` utilisée en instruction est `E-TYP-STATEMENT-VALUE`. `return`/`break`/`continue`/`panic` ont le type `!` (Never), compatible avec tout type attendu. | MUST |
| G-6.6 | `return;` est valide seulement dans une fonction retournant `()` ; sinon `E-TYP-MISMATCH`. `return e` dans une fonction `()` est `E-TYP-MISMATCH`. | MUST |
| G-6.7 | `break;` et `continue;` sont valides seulement dans une boucle (`while`, `loop`, `for`) ; ailleurs `E-TYP-BREAK-OUTSIDE`. `break e;` n'existe pas (`E-PARSE-EXPECTED`). | MUST |
| G-6.8 | L'affectation est une **instruction**, jamais une expression : `if (x = 1) { }` est `E-PARSE-UNEXPECTED`. La **famille complète** des opérateurs composés est acceptée (`+= -= *= /= %= &= |= ^= <<= >>=`) avec la sémantique `place = place op e` (une seule évaluation de `place`). | MUST |
| G-6.9 | La cible d'affectation est un *place* mutable ; `*(&mut x) = e` est valide seulement si le déréférencement porte sur un `&mut T`. Toute autre cible est `E-OWN-IMMUTABLE` ou `E-TYP-MISMATCH`. | MUST |
| G-6.10 | `;;` et `;` seuls sont acceptés et sans effet. | MUST |
| G-6.11 | Un `place` ne peut pas être un littéral, un appel ni une opération : `(a + b).0 = 1;` → `E-PARSE-UNEXPECTED`. | MUST |

```rv
// POSITIF
fn f(mut n: i32) -> i32 {
    let mut s = 0;
    let t: i32;          // initialisation différée
    t = 1;
    s = s + t;
    s += n;
    n = 3;
    if s > 10 { return s; }
    while s > 0 { s += -1; }
    return s;            // tail_expr pourrait être `s` sans `return`
}

// NÉGATIF — E-PARSE-EXPECTED
fn g(t: (i32, i32)) { let (a, b) = t; }
// NÉGATIF — E-PARSE-EXPECTED
fn h(mut x: i32) { x -= 1; }
// NÉGATIF — E-PARSE-UNEXPECTED (affectation en expression)
fn i(x: i32) { if (x = 1) { } }
// NÉGATIF — E-TYP-STATEMENT-VALUE
fn j() { 1 + 1; }
```

### 6.2 Expressions — précédence et associativité

Ordre **croissant** de liaison (le premier lie le plus fort), conforme au design §3.3 :

| Niveau | Opérateurs / formes | Associativité | Non-terminal |
|---|---|---|---|
| 1 | `()`, `[]`, `.`, `?` (postfixes) | gauche | `postfix_expr` |
| 2 | `- ! & &mut *` (préfixes) | droite | `unary_expr` |
| 3 | `as` | gauche | `cast_expr` |
| 4 | `* / %` | gauche | `mul_expr` |
| 5 | `+ -` | gauche | `add_expr` |
| 6 | `<< >>` | gauche | `shift_expr` |
| 7 | `&` (binaire) | gauche | `bitand_expr` |
| 8 | `^` | gauche | `bitxor_expr` |
| 9 | `\|` | gauche | `bitor_expr` |
| 10 | `== != < <= > >=` | **non associative** | `cmp_expr` |
| 11 | `&&` | gauche | `and_expr` |
| 12 | `\|\|` | gauche | `or_expr` |
| 13 | `..` (uniquement en `for`) | non associative | `for_iterable` |
| 14 | affectation (instruction) | — | `assign_stmt` |

| ID | Règle | Niveau |
|---|---|---|
| G-6.12 | `a == b == c` est une erreur de syntaxe (`E-PARSE-UNEXPECTED`), pas une comparaison en chaîne. Les parenthèses sont obligatoires. | MUST |
| G-6.13 | `..` **MUST NOT** apparaître ailleurs que dans la clause `for ... in expr .. expr` : `let r = 0..n;` est `E-UNSUPPORTED-RANGE`. `..=` est `E-UNSUPPORTED-RANGE` (design §3.3 : `0..n` exclusif uniquement). | MUST |
| G-6.14 | `-x as i32` se lit `(-x) as i32` (unaire lie plus fort que `as`). `-x * y` se lit `(-x) * y`. | MUST |
| G-6.15 | `&` unaire et `&` binaire se distinguent **par la position** : en position préfixe (début d'expression ou après un opérateur) c'est l'emprunt ; en position infixe, c'est le `ET` binaire. `a & &b` est valide (`a & (&b)` si `&b: bool`). | MUST |
| G-6.16 | `*` unaire (déréférencement) et `*` binaire (multiplication) se distinguent de la même façon. | MUST |
| G-6.17 | Le `?` postfixe ne s'applique qu'à un `Result`/`Option` et n'est pas chaînable sur un type déjà déballé (contrôlé au typage, `E-TYP-TRY`). | MUST |
| G-6.18 | Aucun opérateur défini par l'utilisateur, aucune surcharge : les opérateurs ci-dessus s'appliquent aux types listés dans [`types.md`](types.md) TYP-7. | MUST |

### 6.3 Expressions

```ebnf
expr           = or_expr ;

or_expr        = and_expr { "||" and_expr } ;
and_expr       = cmp_expr { "&&" cmp_expr } ;
cmp_expr       = bitor_expr [ cmp_op bitor_expr ] ;
cmp_op         = "==" | "!=" | "<" | "<=" | ">" | ">=" ;
bitor_expr     = bitxor_expr { "|" bitxor_expr } ;
bitxor_expr    = bitand_expr { "^" bitand_expr } ;
bitand_expr    = shift_expr { "&" shift_expr } ;
shift_expr     = add_expr { ( "<<" | ">>" ) add_expr } ;
add_expr       = mul_expr { ( "+" | "-" ) mul_expr } ;
mul_expr       = cast_expr { ( "*" | "/" | "%" ) cast_expr } ;
cast_expr      = unary_expr { "as" primitive_type } ;
unary_expr     = ( "-" | "!" | "&" | "&" "mut" | "*" ) unary_expr
               | postfix_expr ;

postfix_expr   = primary { postfix_op } ;
postfix_op     = call_args | index | field | "?" ;
call_args      = "(" [ args ] ")" ;
args           = expr { "," expr } [ "," ] ;
index          = "[" expr "]" ;
field          = "." ( IDENT | INT_LIT ) ;

primary        = literal
               | path_expr
               | "(" expr ")"                       (* groupement *)
               | tuple_expr
               | array_expr
               | struct_expr
               | block
               | if_expr
               | match_expr
               | while_expr
               | loop_expr
               | for_expr ;

tuple_expr     = "(" expr "," [ expr { "," expr } [ "," ] ] ")" ;
array_expr     = "[" [ expr { "," expr } [ "," ] ] "]"
               | "[" expr ";" INT_LIT "]" ;

literal        = INT_LIT | FLOAT_LIT | CHAR_LIT | STRING_LIT | "true" | "false" ;
path_expr      = path ;
path           = path_seg { "::" path_seg } ;
path_seg       = IDENT | "self" | "Self" | "crate" | "super" ;

struct_expr    = type_path "{" [ field_init { "," field_init } [ "," ] ] "}" ;
field_init     = IDENT ":" expr ;

if_expr        = "if" expr block [ "else" ( if_expr | block ) ] ;
match_expr     = "match" expr "{" { match_arm } "}" ;
match_arm      = pattern "=>" ( expr "," | block ) ;
while_expr     = "while" expr block ;
loop_expr      = "loop" block ;
for_expr       = "for" IDENT "in" for_iterable block ;
for_iterable   = or_expr ".." or_expr        (* plage entière exclusive *)
               | expr ;                      (* collection : `Vec<T>` consommé *)
```

| ID | Règle | Niveau |
|---|---|---|
| G-6.19 | `if`/`match` sont des expressions de valeur. `while`/`loop`/`for` sont des **expressions de contrôle** dans l'AST (`While`, `Loop`, `For`) mais leur type est `()` : elles ne produisent pas de valeur. | MUST |
| G-6.20 | Un `if` sans `else` a le type `()` ; un `if` avec `else` a le type de ses branches, qui doivent être identiques (`E-TYP-MISMATCH` sinon). | MUST |
| G-6.21 | Une branche de `match` est soit `expr ,` (virgule obligatoire), soit un bloc. Une branche bloc n'exige pas de virgule ; la virgule y est acceptée et ignorée. | MUST |
| G-6.22 | `match` **MUST** être exhaustif (contrôlé au typage, `E-TYP-NONEXHAUSTIVE`, y compris sur `bool` et les enums ; les entiers exigent `_`). | MUST |
| G-6.23 | Un pattern de `match` ne peut pas porter de garde (`if`) ni de `@` : `E-UNSUPPORTED-TOKEN`, `E-PARSE-EXPECTED`. | MUST |
| G-6.24 | `for` itère soit sur une **plage entière exclusive** `a..b` (bornes de même type entier, `x` de ce type ; `a >= b` ⇒ zéro itération), soit sur une **collection possédée** `Vec<T>` : `for x in v` **déplace** `v` et rend des `T` possédés. Bornes et collection sont évaluées une seule fois. Aucune autre forme d'itération (pas d'itérateur, pas de `&vec`). | MUST |
| G-6.25 | Un littéral de struct ne peut viser qu'un `type_path` nominal local ou importé ; il n'y a pas de syntaxe de struct littéral pour une variante d'enum (utiliser `E::V { ... }`, qui est un `path_expr` suivi de `{`). | MUST |
| G-6.26 | `Point { x: 1.0 }` sans les autres champs est `E-TYP-FIELD` ; un champ inconnu est `E-TYP-FIELD`. Pas de `..base` (struct update syntax) : `E-UNSUPPORTED-RANGE`. | MUST |
| G-6.27 | Un littéral de tableau répété `[x; n]` exige `n` littéral décimal > 0 et un `x` `Copy` (`E-TYP-ARRAY-REPEAT` sinon). | MUST |
| G-6.28 | Un tuple à un élément exige la virgule : `(1,)` est un tuple, `(1)` est un groupement. | MUST |
| G-6.29 | Un `path_expr` peut désigner une fonction, une constante, un type (pour `new`/variante), ou une variante d'enum. Les quatre constructeurs du **prélude** (`Some`, `None`, `Ok`, `Err`) sont acceptés **qualifiés ou non** (`Option::Some(x)` ≡ `Some(x)`). Pour un enum utilisateur, la forme qualifiée est obligatoire (`E-RES-UNKNOWN` sinon). | MUST |
| G-6.30 | L'appel de méthode `receiver.m(args)` est un `postfix_op` ; le receiver peut être une expression arbitraire, y compris un temporaire. L'accès `t.0` est un `field` avec `INT_LIT` uniquement (pas `t.x` sur un tuple, pas `t.0.1` : voir LEX-8.9 et G-6.31). | MUST |
| G-6.31 | `t.0.1` est refusé (`E-PARSE-UNEXPECTED`) : il faut `(t.0).0`. Les indices de champ ne se chaînent pas sans parenthèses. | MUST |
| G-6.32 | Un appel de fonction ordinaire ne peut pas être suivi d'un littéral de bloc ambigu : `f { x }` n'est pas une syntaxe de Core 1.0. Un `struct_expr` exige qu'un type soit identifiable ; dans `if x { }`, `x` n'est jamais traité comme un type de struct. Le parser **désactive** la lecture de `struct_expr` dans la condition d'un `if`/`while`/`match` (règle de lookahead 2, G-6.35). | MUST |
| G-6.33 | Il n'y a **aucune** expression de fermeture (`|x| x + 1`) : `E-UNSUPPORTED-CLOSURE`. | MUST |
| G-6.34 | Il n'y a **aucun** `async`/`await` : `E-UNSUPPORTED-ASYNC`. Aucun `move` : `E-UNSUPPORTED-TOKEN`. | MUST |
| G-6.35 | **Désambiguïsation `Type { ... }` vs bloc** : en position de condition (`if`, `while`, `match` scrutinee), une séquence `path {` est **toujours** interprétée comme la fin de la condition suivie d'un bloc, jamais comme un littéral de struct. Un littéral de struct en condition exige des parenthèses : `if (P { x: 1 }) == p { }`. | MUST |

```rv
// POSITIF
fn f(a: i32, b: i32, p: Point, v: Vec<i32>) -> i32 {
    let c = a + b * 2 - (a << 1) & 0xFF;
    let d = if a > b { a } else { b };
    let e = match p.x { 0.0 => 1, _ => 2 };
    let g = (1, 2,);
    let h = [1, 2, 3];
    let i = [0; 4];
    let j = v[0] + p.y as i32;
    let k = f2(&p, &mut v2);
    return c + d + e + g.0 + h[0] + i[0] + j + k;
}

// NÉGATIF — E-UNSUPPORTED-RANGE
fn r(n: i32) { let x = 0..n; }
// NÉGATIF — E-PARSE-UNEXPECTED
fn s(a: i32, b: i32, c: i32) -> bool { return a < b < c; }
// NÉGATIF — E-RES-UNKNOWN
fn t(o: Option<i32>) -> i32 { match o { Some(x) => x, None => 0 } }
// NÉGATIF — E-UNSUPPORTED-CLOSURE
fn u() { let f = |x: i32| x + 1; }
```

### 6.4 Formes de `match` et bras

```ebnf
pattern        = literal_pat
               | "_"
               | IDENT
               | "(" [ pattern { "," pattern } [ "," ] ] ")"
               | variant_pat
               | "(" pattern ")" ;
variant_pat    = path [ "(" [ pattern { "," pattern } [ "," ] ] ")"
                      | "{" [ pat_field { "," pat_field } [ "," ] ] "}" ] ;
pat_field      = IDENT ":" pattern | IDENT ;        (* raccourci autorisé en pattern *)

literal_pat    = INT_LIT | CHAR_LIT | STRING_LIT | "true" | "false" | "-" INT_LIT ;
```

| ID | Règle | Niveau |
|---|---|---|
| G-6.36 | Un pattern littéral **flottant** est **accepté** : il correspond si la valeur du scrutinee est **égale** (`==`) au littéral. `NaN` ne correspond donc **jamais** (y compris à lui-même) ; `-0.0` et `0.0` correspondent l'un à l'autre. Aucune comparaison de bits n'est faite. | MUST |
| G-6.37 | `-N` est admis en pattern pour un **entier** (`-1`) et pour un **flottant** (`-1.0`). Toute autre forme négative (`-x`, `-(1 + 2)`) est `E-PARSE-EXPECTED`. | MUST |
| G-6.38 | Le pattern `IDENT` lie par valeur (move ou copy). Il n'existe **aucun** pattern `ref`/`&pattern`/`ref mut` : `ref` étant un identifiant valide, `ref x` est un motif de liaison suivi d'un token inattendu (`E-PARSE-EXPECTED`) ; `&x` et `ref mut` sont `E-UNSUPPORTED-TOKEN`. | MUST |
| G-6.39 | Il n'existe **aucun** pattern ou (`p | q`) ni de plage (`1..=5`) : `E-UNSUPPORTED-TOKEN`. | MUST |
| G-6.40 | Le raccourci de champ `E::V { champ }` lie `champ` à `champ`, conformément au design §3.4. Il est **autorisé en pattern seulement**. | MUST |
| G-6.41 | Un pattern de variante d'enum **utilisateur** doit être qualifié (`Shape::Circle(r)`) ; les quatre constructeurs du prélude (`Some`, `None`, `Ok`, `Err`) sont acceptés avec ou sans qualification. Aucune variante n'est importable par `use` ([`modules.md`](modules.md) MOD-3.5). | MUST |
| G-6.42 | Un bras `p => e,` dont `e` est un bloc n'exige pas la virgule ; `p => { ... }` suivi d'un autre bras est valide sans virgule. | MUST |
| G-6.43 | Un bras ne peut pas être vide ; `=> ,` est `E-PARSE-EXPECTED`. | MUST |
| G-6.44 | L'ordre des bras est significatif ; aucun bras n'est réordonné par le compilateur. Un bras inatteignable n'est pas une erreur, n'est pas supprimé et ne produit **aucun diagnostic** (liste d'avertissements fermée). | MUST |

```rv
// POSITIF
fn aire(s: Shape) -> f64 {
    match s {
        Shape::Circle(r) => 3.141592653589793 * r * r,
        Shape::Rect { w: w, h: h } => w * h,
        Shape::Rect { w, h: _ } => w,      // raccourci de champ en pattern
        Shape::Empty => 0.0,
    }
}

// POSITIF — pattern flottant : égalité IEEE, NaN ne correspond jamais
fn g(x: f64) -> i32 { match x { 0.0 => 1, _ => 0 } }
// NÉGATIF — E-UNSUPPORTED-TOKEN
fn h(x: i32) -> i32 { match x { 1 | 2 => 3, _ => 0 } }
// NÉGATIF — E-UNSUPPORTED-TOKEN
fn i(x: i32) -> i32 { match x { n if n > 0 => 1, _ => 0 } }
```

### 6.5 Chemins d'expression

```ebnf
expr_path      = ( "crate" | "super" | "self" | IDENT ) { "::" ( IDENT | "self" | "Self" ) } ;
```

| ID | Règle | Niveau |
|---|---|---|
| G-6.45 | `crate::` désigne la racine du crate courant ; `super::` le module parent ; `self::` le module courant. `self` **chemin** et `self` **paramètre** sont deux tokens de même classe, désambiguïsés par la position (`self::` vs `self`). | MUST |
| G-6.46 | Il n'y a pas d'`use` implicite : tout nom non déclaré localement ou non importé est `E-RES-UNKNOWN`. | MUST |
| G-6.47 | Un chemin peut traverser une dépendance (`util::helper::f`) seulement si la dépendance est déclarée dans `ravum.toml` (`E-RES-UNKNOWN` sinon). | MUST |

## 7. Récapitulatif des constructions refusées syntaxiquement

| Construction | Code |
|---|---|
| `#[...]`, `#![...]` | `E-UNSUPPORTED-ATTR` |
| `trait`, `impl Trait`, `where`, bornes | `E-UNSUPPORTED-TRAIT`, `E-UNSUPPORTED-IMPL-TRAIT`, `E-UNSUPPORTED-WHERE` |
| `dyn T` | `E-RES-UNKNOWN` (`dyn` = identifiant valide) |
| closures `|x| e` | `E-UNSUPPORTED-CLOSURE` |
| `async`, `await`, `move` | `E-UNSUPPORTED-ASYNC`, `-AWAIT`, `-MOVE-KEYWORD` |
| `unsafe`, `extern`, pointeurs bruts | `E-UNSUPPORTED-UNSAFE`, `E-UNSUPPORTED-EXTERN` (`*const`/`*mut` → `E-UNSUPPORTED-UNSAFE`) |
| `macro!`, `macro_rules!` | `E-UNSUPPORTED-MACRO` |
| `static`, `type` alias, `union` | `E-PARSE-EXPECTED` (ce ne sont pas des mots-clés, voir Q-LEX-1) |
| `use a::*`, `use a as b`, `pub use` | `E-UNSUPPORTED-TOKEN`, `-ALIAS`, `-REEXPORT` |
| patterns `ref`, `@`, gardes, `\|` | `E-PARSE-EXPECTED` (`ref` = identifiant), `E-UNSUPPORTED-TOKEN` (garde, `\|`) |
| `..=`, ranges hors `for` | `E-UNSUPPORTED-RANGE`, `-RANGE` |
| `break e` | `E-PARSE-EXPECTED` |
| compound `-= *= /= %= <<= >>= &= ^= |=` | `E-PARSE-EXPECTED` |
| turbofish `f::<T>(..)` | `E-PARSE-UNEXPECTED` |
| shorthand de champ en **expression** | `E-PARSE-EXPECTED` |
| `..base` (struct update) | `E-UNSUPPORTED-RANGE` |

| slicing `&v[1..2]` | `E-UNSUPPORTED-RANGE` |
| `-> impl Trait` | `E-UNSUPPORTED-IMPL-TRAIT` |
| `dyn T` (type) | `E-RES-UNKNOWN` |
| littéraux byte/raw string | `E-UNSUPPORTED-TOKEN` |
| identifiers Unicode | `E-UNSUPPORTED-IDENT` |
| lifetimes `'a` | `E-UNSUPPORTED-TOKEN` |

## 8. Exemples de programmes complets

```rv
// POSITIF — package binaire minimal
use std::io;

fn main() {
    io::println("hello");
}
```

```rv
// POSITIF — module inline, génériques nus, méthodes
mod geo {
    pub struct Point { pub x: f64, pub y: f64 }
    impl Point {
        pub fn new(x: f64, y: f64) -> Point { return Point { x: x, y: y }; }
        pub fn norme(&self) -> f64 { return math::sqrt(self.x * self.x + self.y * self.y); }
    }
    pub fn identite<T>(v: T) -> T { return v; }
}

fn main() {
    let p = geo::Point::new(3.0, 4.0);
    let n = p.norme();
    if n > 0.0 { io::println("ok"); }
}
```

```rv
// NÉGATIF — cumul de diagnostics attendus :
// E-UNSUPPORTED-TRAIT (ligne 2), E-UNSUPPORTED-TOKEN (ligne 9)
trait Affichable { fn affiche(&self) -> String; }

struct P { x: i32 }

fn f(p: P) -> i32 {
    match p.x { n if n > 0 => 1, _ => 0 }
}
```

## 9. Questions ouvertes

- **Q-GRAM-1 (résolue par le Lead)** — G-6.1 autorise l'omission de `;` après une expression block_like en position d'instruction (comme Rust) tout en restant **déterministe** : la forme syntaxique de l'expression et le token suivant (`;`, `}` ou autre) suffisent, sans dépendre du typage. Un `while`/`if` en milieu de bloc n'exige donc pas `;` ; suivi de `;`, il reste une instruction.
- **Q-GRAM-2 (bloquante pour la cohérence)** — Le design §3.1 écrit `struct Point { pub x: f64, y: f64 }` **et** « `pub` sur items, champs et variantes ». Or `pub` sur une variante n'a pas de sens (pas de variante privée en Core 1.0). G-4.2 l'accepte sans effet. Faut-il interdire `pub` sur une variante (`E-PARSE-EXPECTED`) ou le tolérer silencieusement ? Recommandation : accepter sans effet et le documenter, pour ne pas casser les corpus.
- **Q-GRAM-3 (résolue par le Lead)** — La famille **complète** des opérateurs composés est acceptée (`+= -= *= /= %= &= |= ^= <<= >>=`), corrigeant la limitation du design §3.3 qui ne montrait que `+=`. Aucun `E-*` spécifique : une forme invalide (`**=`) est `E-PARSE-UNEXPECTED`.
- **Q-GRAM-4** — G-5.13 interdit les `const` dépendant d'autres `const`. La plupart des langages l'autorisent. Refus conservateur en attendant une décision.
- **Q-GRAM-5 (résolue)** — Les patterns de littéraux flottants sont **acceptés** (G-6.36) avec la sémantique `==` : `NaN` ne correspond jamais, `-0.0` correspond à `0.0`. Aucun besoin de bits.
- **Q-GRAM-6** — `match` sur `&str` (littéraux de chaîne en pattern) est autorisé par G-6.36, mais le design §3.4 dit seulement « littéraux ». Confirmer que les patterns `&str` sont dans le périmètre (ils n'exigent aucune égalité de trait).
- **Q-GRAM-7** — G-6.25/G-6.32 (désambiguïsation struct-literal) est une règle syntaxique imposée par l'absence de traits et par le lookahead. Vérifier qu'elle ne rend pas `if p == Point { x: 1 } { }` surprenant ; les parenthèses sont exigées.
- **Q-GRAM-8** — `test fn` : le design écrit `test fn nom() { }`. La position de `test` avant `fn` est retenue. Faut-il aussi accepter `fn nom() test` ou un attribut `#[test]` (refusé, attributs UNSUPPORTED) ? Non tranché par le design.
- **Q-GRAM-9** — G-5.5 interdit `[T; 0]`. Cela empêche les tableaux vides utiles au code DSP générique. À confirmer.
- **Q-GRAM-10** — `!` est désormais nommable (G-5.11) et coercible vers tout type, mais reste interdit en paramètre, champ, élément de `Vec` et retour d'une fonction non divergente. Confirmer cette restriction : elle évite les signatures « qui ne rendent jamais » ambiguës.
