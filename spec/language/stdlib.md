# Ravum Core 1.0 — Prélude et bibliothèque standard (API exacte)

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.8.** Les signatures ci-dessous intègrent les décisions du Lead (2026-11-09) ; elles sont **normatives**.
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR** et ne certifie pas la
> conformité de l'implémentation. Les fonctions décrites ci-dessous **sont implémentées et
> exercées** sur le sous-ensemble Core 1.0 (cas `cases/run/*` et `features.md` F-STD-*) ;
> leur état exact est tenu par la conformance, pas par ce document.
> Effets : [`effects.md`](effects.md) ; types : [`types.md`](types.md) ; codes : [`diagnostics.md`](diagnostics.md) ; exemples réels : `examples/` (hello, fib, calculator, file-tool, stats, dsp-gain-iir, package-demo, tests-demo).

## 1. Prélude, imports, règles générales

### 1.1 Prélude (disponible sans `use`)

| Catégorie | Noms |
|---|---|
| Types primitifs | `i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 isize usize f32 f64 bool char ()`, `!` (Never) |
| Types du prélude | `String`, `Vec<T>`, `Option<T>`, `Result<T, E>`, `Error` |
| Constructeurs | `Some`, `None`, `Ok`, `Err` (qualifiés ou non, [`types.md`](types.md) TYP-10.2) |
| Intrinsèques | `assert`, `assert_eq`, `panic`, `drop` |

### 1.2 Modules (toujours importés par `use`)

| Module | Import | Contenu |
|---|---|---|
| `std::io` | `use std::io;` | `print`, `println`, `eprintln`, `read_line` |
| `std::fs` | `use std::fs;` | `read_to_string`, `write_string`, `exists` |
| `std::env` | `use std::env;` | `args`, `var` |
| `std::process` | `use std::process;` | `exit` |
| `std::math` | `use std::math;` | `abs`, `sqrt`, `floor`, `ceil`, `round`, `pow`, `min`, `max` (f64) |
| `std::string` | `use std::string;` | `from`, `parse_i64`, `parse_f64` |
| `std::panic` | `use std::panic;` | `panic`, `assert`, `assert_eq` (ré-exposés par le prélude) |

| ID | Règle | Niveau |
|---|---|---|
| STD-1.1 | Aucun **module** n'est dans le prélude : `io::println` exige `use std::io;`. Écrire `io.println(...)` (point) est `E-RES-UNKNOWN` : il n'y a pas d'appel de méthode sur un module ([`modules.md`](modules.md) MOD-6.7, [`unsupported.md`](unsupported.md)). | MUST |
| STD-1.2 | `std` est fourni par le compilateur : non déclarable, non verrouillé, non remplaçable, non versionné par l'utilisateur ([`modules.md`](modules.md) §8.3). | MUST |
| STD-1.3 | Cette liste est **fermée** : tout nom non listé ici est `E-RES-UNKNOWN`. Aucune fonction `std` n'est variadique, n'a de paramètre nommé ou par défaut, ni de fermeture. | MUST |
| STD-1.4 | Les signatures sont **normatives** (nom, ordre, types, retour, effets). Un écart est un défaut de conformité. | MUST |
| STD-1.5 | Les méthodes listées sont **inhérentes** ; aucun `impl` utilisateur sur les types du prélude (`E-RES-IMPL-SELF-TYPE`). | MUST |
| STD-1.6 | Deux ajustements implicites seulement, en **argument ou récepteur** : `&[T; N] → &[T]` et `&String → &str` ([`types.md`](types.md) TYP-13.1). Aucune autre conversion implicite. | MUST |
| STD-1.7 | Les messages d'erreur (champ `message` d'`Error`) sont **illustratifs et non stables** ; seul le fait de rendre `Result::Err` est normatif (Q-STD-5). | MUST |
| STD-1.8 | Toute fonction `alloc` **MUST** être considérée allouante même si un chemin donné n'alloue pas (granularité fonction, EFF-2.5). | MUST |

```rv
// POSITIF
use std::io;
use std::math;

fn main() {
    io::println("hello, ravum");
    let x = math::sqrt(2.0);
}

// NÉGATIF — E-RES-UNKNOWN (forme point interdite)
use std::io;
fn main2() { io.println("hello"); }
// NÉGATIF — E-RES-UNKNOWN (module non importé)
fn main3() { io::println("hello"); }
```

## 2. `std::io`

```rv
pub fn print(s: &str)    effects(io)
pub fn println(s: &str)  effects(io)
pub fn eprintln(s: &str) effects(io)
pub fn read_line() -> Result<String, Error> effects(io, alloc)
```

| ID | Sémantique | Niveau |
|---|---|---|
| STD-2.1 | `print(s)` écrit les **octets UTF-8** de `s` sur stdout : aucun ajout, aucun formatage, aucune traduction de fin de ligne. | MUST |
| STD-2.2 | `println(s)` écrit `s` puis **un seul** octet `0x0A` (`\n`), **jamais** `\r\n`, y compris sous Windows. | MUST |
| STD-2.3 | `eprintln(s)` écrit `s` + `0x0A` sur stderr. Il n'existe pas de `eprint`. | MUST |
| STD-2.4 | Aucune interpolation : `println("a = {}", n)` est un appel à **deux** arguments pour un paramètre → `E-TYP-ARITY`. Convertir avec `to_string()` ou [`std::string`](#7-stdstring). | MUST |
| STD-2.5 | Erreur d'écriture (`EPIPE`, disque plein, descripteur fermé) → **panic** (`panic: failed printing to stdout`), non modélisé comme effet (`effects.md` EFF-6.4). | MUST |
| STD-2.6 | Pas de fonction `flush`. Le tampon de stdout peut être vidé par ligne (terminal) ou à la sortie ; aucun ordre relatif stdout/stderr n'est spécifié. | MUST |
| STD-2.7 | `read_line()` lit jusqu'à `\n` **inclus** et rend la ligne **sans** terminateur (`\n` retiré ; `\r\n` : les deux retirés). | MUST |
| STD-2.8 | EOF **sans aucun octet** → `Err(Error::new("unexpected end of input"))` ; EOF après des octets → `Ok(ligne)`. Octets non UTF-8 → `Err` (aucun remplacement). | MUST |
| STD-2.9 | `read_line()` alloue toujours un nouveau `String`, jamais réutilisé. | MUST |

```rv
// POSITIF
use std::io;

fn main() -> Result<(), Error> {
    io::println("entrez un nombre :");
    let l = io::read_line()?;
    io::println(l.trim().as_str());
    return Ok(());
}
```

```rv
// NÉGATIF — E-TYP-ARITY
fn f(n: i64) { io::println("n = {}", n); }
// NÉGATIF — E-EFF-PROFILE (io sous `pure`)
fn main() { io::println("x"); }
```

## 3. `std::fs`

```rv
pub fn read_to_string(path: &str) -> Result<String, Error> effects(fs, io, alloc)
pub fn write_string(path: &str, contents: &str) -> Result<(), Error> effects(fs, io)
pub fn exists(path: &str) -> bool effects(fs)
```

| ID | Sémantique | Niveau |
|---|---|---|
| STD-3.1 | Chemins = chaînes UTF-8 interprétées par l'OS ; **aucune** normalisation, aucune restriction d'accès (`../x`, absolus, symlinks acceptés à l'exécution). | MUST |
| STD-3.2 | `read_to_string` lit **tout** le fichier et exige de l'**UTF-8 strict** ; aucune conversion de fin de ligne (`\r\n` conservé). | MUST |
| STD-3.3 | `write_string` **crée ou tronque**, puis écrit les octets UTF-8. Aucun répertoire intermédiaire n'est créé. | MUST |
| STD-3.4 | `write_string` **n'est pas atomique** : pas de fichier temporaire, pas de `rename`, pas de `fsync`. Une erreur peut laisser un fichier **partiel ou vide** ; aucune garantie de durabilité. | MUST |
| STD-3.5 | `exists` rend `true` si un fichier **ou** répertoire existe, `false` sinon, **y compris** sur erreur d'accès (permission refusée → `false`). Limitation assumée (Q-STD-2). | MUST |
| STD-3.6 | Aucun mode d'ouverture, append, permission, verrou ni lecture partielle. | MUST |

```rv
// POSITIF
use std::fs;
use std::io;

fn main() -> Result<(), Error> {
    if !fs::exists("out.txt") {
        fs::write_string("out.txt", "bonjour\n")?;
    }
    let s = fs::read_to_string("out.txt")?;
    io::println(s.as_str());
    return Ok(());
}
```

```rv
// NÉGATIF — E-TYP-ARITY (argument manquant)
fn f() { fs::write_string("x"); }
// NÉGATIF — E-EFF-PROFILE (fs sous `rt`)
fn g() effects() { let b = fs::exists("x"); }
```

## 4. `std::env`

```rv
pub fn args() -> Vec<String> effects(io, alloc)
pub fn var(name: &str) -> Result<String, Error> effects(io, alloc)
```

| ID | Sémantique | Niveau |
|---|---|---|
| STD-4.1 | `args()[0]` est le nom du programme tel qu'invoqué ; `args()[1..]` les arguments dans l'ordre. Le `Vec` est neuf à chaque appel. | MUST |
| STD-4.2 | Un argument non UTF-8 provoque `panic: non-UTF-8 argument` (pas de remplacement, pas de perte silencieuse — Q-STD-3). | MUST |
| STD-4.3 | `var(name)` rend la valeur de la variable d'environnement `name`, ou `Err` si absente (ou non UTF-8). Variable vide `""` valide → `Ok("")`. | MUST |
| STD-4.4 | Le groupe d'effet de `args`/`var` est `io` + `alloc` (aucun groupe `env`/`process` n'existe ; Q-EFF-5). | MUST |
| STD-4.5 | Aucune écriture d'environnement, aucun `current_dir`, aucun `var_os`. | MUST |

```rv
// POSITIF
use std::env;
use std::io;

fn main() {
    let a = env::args();
    if a.len() > 1 { io::println(a[1].as_str()); }
    match env::var("RAVUM_MODE") {
        Result::Ok(v) => io::println(v.as_str()),
        Result::Err(e) => io::eprintln(e.message.as_str()),
    }
}
```

## 5. `std::process`

```rv
pub fn exit(code: i32) -> ! effects(io)
```

| ID | Sémantique | Niveau |
|---|---|---|
| STD-5.1 | Termine immédiatement le processus ; type de retour `!` (Never), utilisable comme expression de tout type. | MUST |
| STD-5.2 | Aucun destructeur n'est exécuté, aucune valeur n'est détruite : l'ordre de drop (OWN-8) est ignoré. | MUST |
| STD-5.3 | Les tampons sont vidés au mieux (*best effort*), sans garantie. | MUST |
| STD-5.4 | Le code effectif dépend de la plateforme (POSIX : 8 bits ; Windows : 32 bits) : `exit(256)` → `0` sous POSIX. Comportement **spécifié comme dépendant de la plateforme**. | MUST |
| STD-5.5 | `exit(1)` est indistinguable d'un panic pour un script ([`diagnostics.md`](diagnostics.md) §8). | MUST |

```rv
// POSITIF
use std::process;

fn main() {
    process::exit(3);
}
```

## 6. `std::math` (f64 uniquement)

```rv
pub fn abs(x: f64) -> f64
pub fn sqrt(x: f64) -> f64
pub fn floor(x: f64) -> f64
pub fn ceil(x: f64) -> f64
pub fn round(x: f64) -> f64
pub fn pow(a: f64, b: f64) -> f64
pub fn sin(x: f64) -> f64
pub fn cos(x: f64) -> f64
pub fn tan(x: f64) -> f64
pub fn exp(x: f64) -> f64
pub fn ln(x: f64) -> f64
pub fn log10(x: f64) -> f64
pub fn min(a: f64, b: f64) -> f64
pub fn max(a: f64, b: f64) -> f64
```
Toutes `effects()` (pures).

| ID | Sémantique | Niveau |
|---|---|---|
| STD-6.1 | Aucune variante `f32` : convertir par `as` (TYP-13.2), avec perte documentée. | MUST |
| STD-6.2 | `sqrt` est **correctement arrondi** IEEE-754 : `sqrt(-0.0) = -0.0`, `sqrt(x < 0) = NaN`, `sqrt(+inf) = +inf`. | MUST |
| STD-6.3 | `floor`, `ceil` sont **exacts** ; `round` arrondit à l'entier le plus proche, **égalité loin de zéro** (`0.5 → 1.0`, `-0.5 → -1.0`) ; `-0.0` est préservé par `floor`/`ceil`/`round`. | MUST |
| STD-6.4 | `abs(-0.0) = +0.0`, `abs(NaN) = NaN`, `abs(±inf) = +inf`. | MUST |
| STD-6.5 | `min`/`max` : si un opérande est `NaN`, l'autre est rendu ; si les deux sont `NaN`, `NaN` ; `min(-0.0, +0.0) = -0.0`, `max(-0.0, +0.0) = +0.0`. | MUST |
| STD-6.6 | `pow(a,b)`, `sin`, `cos`, `tan`, `exp`, `ln`, `log10` suivent l'implémentation `libm` de la plateforme : **précision non garantie**, non correctement arrondis, dépendants du CPU/OS. Ils sont **exclus des tests bit-exacts** (Q-STD-4) ; les cas de conformance n'assertent que des valeurs exactement représentables ou des tolérances explicites. | MUST |
| STD-6.7 | Ne sont **pas** fournies : `asin`, `acos`, `atan`, `atan2`, `log2`, `powi`, `hypot`, `sinh`, `cosh`, `tanh`, `cbrt`, `trunc`, `signum`, `copysign`, `is_nan`, `is_finite`, `is_infinite`, `NAN`, `INFINITY`, `PI`, `E`. `NaN` s'obtient par `0.0/0.0`, `+inf` par `1.0/0.0`. | MUST |
| STD-6.8 | Les fonctions trigonométriques et exponentielles existent donc dès Core 1.0, mais **sans garantie de précision** : un résultat dépendant de la `libm` ne doit jamais être asserté bit-à-bit. Les cas DSP déterministes (`+`, `*`) restent la référence des tests. | MUST |

```rv
// POSITIF
use std::math;
use std::io;

fn main() {
    io::println(math::sqrt(2.0).to_string().as_str());
    let lo = math::min(-0.0, 0.0);          // -0.0
    let rounded = math::round(-0.5);        // -1.0
    let p = math::pow(2.0, 10.0);           // 1024.0 (précision libm, non bit-exacte)
}
```

```rv
// NÉGATIF — E-RES-UNKNOWN (non fournie)
fn f(x: f64) -> f64 { return math::sin(x); }
// NÉGATIF — E-RES-UNKNOWN (pas de constante PI)
fn g() -> f64 { return math::PI; }
```

## 7. `std::string`

```rv
pub fn from(s: &str) -> String effects(alloc)
pub fn parse_i64(s: &str) -> Result<i64, Error> effects(alloc)
pub fn parse_f64(s: &str) -> Result<f64, Error> effects(alloc)
```

| ID | Sémantique | Niveau |
|---|---|---|
| STD-7.1 | `string::from(s)` ≡ `String::from(s)` : copie possédée du texte. | MUST |
| STD-7.2 | `parse_i64` : la chaîne est consommée **entièrement** ; aucun blanc en tête/queue ; signe `+`/`-` optionnel ; au moins un chiffre décimal ; aucun `_`, aucun préfixe de base, aucun suffixe ; **dépassement = `Err`** (pas de saturation). | MUST |
| STD-7.3 | `parse_f64` : signe optionnel ; mantisse avec partie entière **et/ou** fractionnaire (au moins un chiffre) ; exposant `e`/`E` optionnel ; accepte **insensible à la casse** `inf`, `infinity`, `nan`. Refuse `1.` et `.5`. Dépassement vers `±inf` = `Err` ; sous-dépassement vers `0` accepté. | MUST |
| STD-7.4 | Chaîne vide → `Err`. Aucun remplacement, aucune réparation. | MUST |
| STD-7.5 | Le `Error` ne porte **aucun code** : seule `message` existe, et son texte n'est pas stable (STD-1.7). Un programme ne peut pas distinguer « format invalide » de « dépassement » de façon stable (Q-STD-5). | MUST |
| STD-7.6 | Les conversions *vers* texte sont les méthodes `to_string()` des primitives (§12), pas des fonctions de module. | MUST |

```rv
// POSITIF
use std::string;
use std::io;

fn main() -> Result<(), Error> {
    let n = string::parse_i64("-42")?;
    let x = string::parse_f64("1.5e-3")?;
    io::println(n.to_string().as_str());
    io::println(x.to_string().as_str());
    let s = string::from("ravum");
    return Ok(());
}
```

```rv
// NÉGATIF — E-TYP-MISMATCH (String au lieu de &str ; pas de coercition String → &str par valeur)
fn f(s: String) -> Result<i64, Error> { return string::parse_i64(s); }
```

## 8. `std::panic` et intrinsèques

```rv
pub fn panic(msg: &str) -> ! effects(panic)
pub fn assert(cond: bool) effects(panic)

// Intrinsèque (forme de surface) : deux arguments évalués par valeur, comparés par égalité intégrée.
pub fn assert_eq(a: T, b: T) effects(panic)
// ABI interne : les deux opérandes sont matérialisés puis passés par référence (&T, &T).
```

| ID | Sémantique | Niveau |
|---|---|---|
| STD-8.1 | `assert(false)` panic : `panic: assertion failed: <file>:<line>:<col>` (coordonnées **Ravum**, pas Rust). Message sur stderr, code de sortie `1`, aucune trace de pile. | MUST |
| STD-8.2 | `assert_eq(a, b)` : arguments **par valeur** (la signature interne `assert_eq<T>(&T, &T)` est un détail d'ABI, pas la syntaxe d'appel). Types admis : tous les entiers, `f32`/`f64`, `bool`, `char`, et le **texte** (`String`, `&str`, mélange) — mêmes règles que `==` (TYP-7.3). Sinon `E-TYP-EQ`. | MUST |
| STD-8.3 | Échec de `assert_eq` : `panic: assertion failed: <file>:<line>:<col>: left = <a>, right = <b>`. Les scalaires sont formatés selon §12 ; le texte est inséré brut ; les flottants suivent la règle `to_string` de §12. | MUST |
| STD-8.4 | `panic(msg)` : `panic: <msg>` sur stderr, code `1`, type `!`. | MUST |
| STD-8.5 | `assert`, `assert_eq`, `panic` sont les **seules** sources de panic par appel explicite ; leur effet est `panic` et contamine les appelants. Les traps intrinsèques (overflow, division par zéro, index, décalage) ne sont **pas** suivis (EFF-6.4). | MUST |
| STD-8.6 | `assert(msg)` à deux arguments (message) **n'existe pas** en Core 1.0 : `assert(bool)` seulement (Q-STD-8). | MUST |
| STD-8.7 | Il n'existe ni `debug_assert`, `assert_ne`, `todo`, `unimplemented`, `unreachable`, `dbg`, ni `catch_unwind`. | MUST |

```rv
// POSITIF
use std::panic;

test fn addition_works() {
    assert_eq(1 + 1, 2);
    assert(true);
    assert_eq("ravum".to_uppercase(), "RAVUM");
}

// NÉGATIF — E-TYP-EQ (assert_eq sur un type non comparable)
fn f() { assert_eq(Vec::<i32>::new(), Vec::<i32>::new()); }
// NÉGATIF — E-TYP-ARITY
fn g() { assert_eq(1); }
```

## 9. Méthodes de texte : `String` et `&str` (`Type::Str`)

Les méthodes suivantes existent **à l'identique** sur `String` et sur `&str` (texte non possédé, [`types.md`](types.md) TYP-6.3), sauf `as_str` (spécifique à `String`) et les constructeurs.

```rv
impl String {
    pub fn new() -> String effects(alloc)
    pub fn from(s: &str) -> String effects(alloc)
    pub fn as_str(&self) -> &str effects()
}

impl String /* et Str */ {
    pub fn len(&self) -> usize effects()
    pub fn is_empty(&self) -> bool effects()
    pub fn push_str(&mut self, s: &str) effects(alloc)
    pub fn push(&mut self, c: char) effects(alloc)
    pub fn clone(&self) -> String effects(alloc)
    pub fn trim(&self) -> String effects(alloc)
    pub fn to_uppercase(&self) -> String effects(alloc)
    pub fn to_lowercase(&self) -> String effects(alloc)
    pub fn contains(&self, pat: &str) -> bool effects()
    pub fn replace(&self, from: &str, to: &str) -> String effects(alloc)
    pub fn lines(&self) -> Vec<String> effects(alloc)
    pub fn starts_with(&self, pat: &str) -> bool effects()
    pub fn ends_with(&self, pat: &str) -> bool effects()
    pub fn parse_i64(&self) -> Result<i64, Error> effects(alloc)
    pub fn parse_f64(&self) -> Result<f64, Error> effects(alloc)
}
```

| ID | Sémantique | Niveau |
|---|---|---|
| STD-9.1 | **Toutes les opérations texte rendent un `String` possédé** (`trim`, `to_uppercase`, `to_lowercase`, `replace`, `lines`). C'est le prix du modèle sans lifetimes : aucune référence n'est stockée ([`ownership.md`](ownership.md) OWN-6). **Coût** : une allocation par appel ; à documenter dans les guides et à mesurer (aucun chiffre n'est publié ici). | MUST |
| STD-9.2 | `len()` = nombre d'**octets** UTF-8 (pas de caractères, pas de graphèmes). `is_empty()` ⇔ `len() == 0`. | MUST |
| STD-9.3 | `as_str()` rend une `&str` empruntée : **non stockable, non retournable**, utilisable dans l'instruction (argument, comparaison, récepteur) — OWN-6. | MUST |
| STD-9.4 | `push(c)` ajoute l'encodage UTF-8 de `c` ; `push_str(s)` ajoute les octets de `s`. Le résultat est **toujours** de l'UTF-8 valide. | MUST |
| STD-9.5 | `trim()` retire les blancs Unicode en tête et en queue (mêmes blancs que `char::is_whitespace` de Rust) et rend une copie. | MUST |
| STD-9.6 | `to_uppercase`/`to_lowercase` suivent le mapping Unicode simple de Rust ; la longueur en octets peut changer. | MUST |
| STD-9.7 | `contains`/`starts_with`/`ends_with` comparent des **octets** UTF-8 (recherche de sous-séquence), sans normalisation. | MUST |
| STD-9.8 | `replace(from, to)` remplace **toutes** les occurrences non chevauchantes de `from` ; `from` vide insère `to` entre chaque caractère (même comportement que Rust `str::replace`). | MUST |
| STD-9.9 | `lines()` découpe sur `\n` (et `\r\n`), **sans** le terminateur ; une dernière ligne vide après un `\n` final n'est **pas** produite ; rend `Vec<String>` possédé. | MUST |
| STD-9.10 | `parse_i64`/`parse_f64` sont les méthodes équivalentes à [`std::string`](#7-stdstring) : mêmes règles exactes. | MUST |
| STD-9.11 | **Aucune autre méthode** n'existe : pas de `chars`, `bytes`, `split`, `join`, `to_string` (les primitives l'ont, pas le texte : utiliser `clone`/`from`), `insert`, `remove`, `find`, `rfind`, `char_at`, `repeat`, `clear`, `with_capacity`, `truncate`, `pop`, `as_bytes`. Utilisation → `E-RES-UNKNOWN`. | MUST |
| STD-9.12 | Un `String` n'est **pas** indexable (`s[0]` → `E-TYP-INDEX`) et n'a **pas** d'itérateur. | MUST |

```rv
// POSITIF (tiré de examples/stats et tests-demo)
use std::string;
use std::io;

fn main() -> Result<(), Error> {
    let text = String::from("1.0\n2.0\n3.0\n");
    let lines = text.lines();                 // Vec<String> possédé
    let mut sum = 0.0;
    let mut count = 0;
    for line in lines {
        let trimmed = line.trim();            // String possédé
        if trimmed.is_empty() { continue; }
        sum += string::parse_f64(trimmed.as_str())?;
        count += 1;
    }
    io::println((sum / (count as f64)).to_string().as_str());
    return Ok(());
}

test fn strings_work() {
    let s = String::from("ravum");
    assert_eq(s.len(), 5);
    assert(s.starts_with("ra"));
    assert_eq(s.to_uppercase(), "RAVUM");     // String vs &str : égalité texte
}
```

```rv
// NÉGATIF — E-RES-UNKNOWN (méthode non fournie)
fn f(s: String) -> usize { return s.chars().len(); }
// NÉGATIF — E-TYP-INDEX
fn g(s: String) -> u8 { return s[0]; }
```

## 10. `Vec<T>`

```rv
impl<T> Vec<T> {
    pub fn new() -> Vec<T> effects(alloc)
    pub fn with_capacity(n: usize) -> Vec<T> effects(alloc)
    pub fn len(&self) -> usize effects()
    pub fn is_empty(&self) -> bool effects()
    pub fn push(&mut self, x: T) effects(alloc)
    pub fn pop(&mut self) -> Option<T> effects()
    pub fn clear(&mut self) effects()
    pub fn clone(&self) -> Vec<T> effects(alloc)   // disponible seulement si T: Copy
    pub fn as_slice(&self) -> &[T] effects()       // vue &[T] utilisable dans l'instruction
}

impl<T> [T] /* via &[T] */ {
    pub fn len(&self) -> usize effects()
    pub fn is_empty(&self) -> bool effects()
}
```

| ID | Sémantique | Niveau |
|---|---|---|
| STD-10.1 | `new()` crée un vecteur vide ; `T` doit être inféré du contexte (`E-TYP-AMBIGUOUS` sinon, TYP-13.4). | MUST |
| STD-10.2 | `with_capacity(n)` crée un vecteur vide avec une capacité **au moins** `n` (aucune garantie exacte, aucune observable). | MUST |
| STD-10.3 | `push(x)` ajoute `x` en fin (déplacement si mobile) ; `pop()` retire et rend le dernier élément ou `None` ; `clear()` retire tout (`len() == 0`), détruit les éléments mobiles, sans effet `alloc`. | MUST |
| STD-10.4 | `clone()` n'est disponible que si `T` est `Copy` (règle structurelle, pas une borne de trait) ; sinon `E-RES-UNKNOWN`. Reconstruire pour les non-`Copy`. | MUST |
| STD-10.5 | `v[i]` exige `i: usize`, panic si `i >= len` ; lire un `T` non-`Copy` en valeur est `E-OWN-PARTIAL-MOVE` ([`types.md`](types.md) TYP-9.3). | MUST |
| STD-10.6 | `&[T]` offre `len`, `is_empty` et `s[i]` (lecture, panic hors bornes) ; obtenue par coercition `&[T; N] → &[T]` en argument. | MUST |
| STD-10.7 | `as_slice()` rend une `&[T]` empruntée, utilisable dans l'instruction (argument, indexation, `len`) et **non stockable** (OWN-6). C'est le **seul** moyen d'obtenir une `&[T]` depuis un `Vec` ; il n'existe **aucune** coercition `&Vec<T> → &[T]`. | MUST |
| STD-10.8 | **Aucune autre méthode** : pas de `get`, `insert`, `remove`, `retain`, `iter`, `map`, `filter`, `sort`, `contains`, `extend`, `truncate`, `resize`, `reverse`, `dedup`, `first`, `last`. | MUST |

```rv
// POSITIF (tiré de examples/dsp-gain-iir)
fn gain(samples: &[f64], g: f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(samples.len());
    let mut i: usize = 0;
    while i < samples.len() {
        out.push(samples[i] * g);
        i += 1;
    }
    out
}

fn main() {
    let input = [1.0, 0.5, -0.25, 0.0];
    let boosted = gain(&input, 2.0);      // &[f64; 4] -> &[f64]
    let n = boosted.len();
}
```

```rv
// NÉGATIF — E-RES-UNKNOWN (clone d'un Vec de non-Copy)
fn f(v: Vec<String>) -> Vec<String> { return v.clone(); }
// NÉGATIF — E-RES-UNKNOWN (méthode absente)
fn g(v: Vec<i32>) -> Option<i32> { return v.get(0); }
```

## 11. `Option<T>` et `Result<T, E>`

```rv
impl<T> Option<T> {
    pub fn is_some(&self) -> bool effects()
    pub fn is_none(&self) -> bool effects()
    pub fn unwrap(self) -> T effects(panic)
    pub fn unwrap_or(self, default: T) -> T effects()
    pub fn ok_or<E>(self, err: E) -> Result<T, E> effects()
}

impl<T, E> Result<T, E> {
    pub fn is_ok(&self) -> bool effects()
    pub fn is_err(&self) -> bool effects()
    pub fn unwrap(self) -> T effects(panic)
    pub fn unwrap_or(self, default: T) -> T effects()
    pub fn ok(self) -> Option<T> effects()
    pub fn err(self) -> Option<E> effects()
}
```

| ID | Sémantique | Niveau |
|---|---|---|
| STD-11.1 | Les prédicats (`is_*`) ne déplacent rien, n'ont aucun effet. | MUST |
| STD-11.2 | `Option::unwrap` sur `None` : `panic: called Option::unwrap on None`. `Result::unwrap` sur `Err` : `panic: called Result::unwrap on an Err value` (contenu non formaté). | MUST |
| STD-11.3 | `unwrap_or(self, d)` rend `T` ou `d` (déplacé) ; `ok_or(self, e)` convertit `Option` en `Result` ; `ok`/`err` projettent `Result` en `Option`. Tous consomment le receveur. | MUST |
| STD-11.4 | Aucune autre méthode : pas de `map`, `and_then`, `or_else`, `expect`, `take`, `replace`, `iter`, `collect`. Un `match` ou `?` est requis. | MUST |
| STD-11.5 | `?` n'est pas une méthode : voir [`expressions.md`](expressions.md) §8. | MUST |

```rv
// POSITIF
fn f(o: Option<i32>) -> i32 { return o.unwrap_or(0); }
fn g(r: Result<i32, Error>) -> Option<i32> { return r.ok(); }
fn h(o: Option<i32>) -> Result<i32, Error> { return o.ok_or(Error::new("vide")); }
```

## 12. Primitives : `to_string`, `f64`, entiers signés

```rv
// Méthodes sur chaque primitive (entiers, isize/usize, f32, f64, bool, char)
pub fn to_string(&self) -> String effects(alloc)

impl f64 {
    pub fn abs(self) -> f64 effects()
    pub fn sqrt(self) -> f64 effects()
    pub fn floor(self) -> f64 effects()
    pub fn ceil(self) -> f64 effects()
    pub fn round(self) -> f64 effects()
    pub fn powf(self, y: f64) -> f64 effects()
    pub fn min(self, other: f64) -> f64 effects()
    pub fn max(self, other: f64) -> f64 effects()
}

// Entiers signés uniquement : i8 i16 i32 i64 i128 isize
pub fn abs(self) -> Self effects()
```

| ID | Sémantique | Niveau |
|---|---|---|
| STD-12.1 | `to_string()` : entiers en décimal (signe `-` si négatif, aucun `+`, aucun zéro de tête sauf `0`, aucun séparateur) ; `bool` → `"true"`/`"false"` ; `char` → le caractère seul, sans guillemets. | MUST |
| STD-12.2 | `f32`/`f64::to_string()` = **`Display` de Rust** : plus courte représentation décimale qui, relue, redonne exactement la valeur ; `NaN` → `"NaN"`, `+inf` → `"inf"`, `-inf` → `"-inf"`, `-0.0` → `"-0"`, `0.0` → `"0"`. Cette règle est **déterministe** et fait partie de la spécification (Q-STD-4). | MUST |
| STD-12.3 | Les méthodes `f64` sont équivalentes aux fonctions `math` (§6), sauf `powf(y)` qui correspond à `math::pow(self, y)` — **précision non garantie** (libm). | MUST |
| STD-12.4 | `abs()` sur un entier signé égale la valeur absolue ; sur `MIN` (`i8::MIN` … `i128::MIN`), elle **déborde** → panic contrôlé (trap intrinsèque, non suivi comme effet). | MUST |
| STD-12.5 | `u8…u128`, `usize`, `bool`, `char` n'ont **pas** de méthode `abs()` (`E-RES-UNKNOWN`). | MUST |
| STD-12.6 | Aucune autre méthode primitive : pas de `pow`, `count_ones`, `wrapping_add`, `checked_add`, `saturating_*`, `clamp`, `is_nan`, `to_bits`, `from_bits`, `parse` (utiliser `std::string`). | MUST |

```rv
// POSITIF
fn f(x: f64, n: i64) -> String {
    let a = x.abs().sqrt();               // méthode f64
    let b = n.abs();                      // entier signé
    let mut s = a.to_string();            // astuce : construire par mutation
    s.push_str(" unit");
    return s;
}
```

```rv
// NÉGATIF — E-TYP-EQ / opérateur absent : `+` sur String n'existe pas
fn h(a: String, b: String) -> String { return a + b; }
// NÉGATIF — E-RES-UNKNOWN (abs sur non signé)
fn i(x: u32) -> u32 { return x.abs(); }
```

## 13. `Error`

```rv
pub struct Error { pub message: String }

impl Error {
    pub fn new(message: &str) -> Error effects(alloc)
}
```

| ID | Sémantique | Niveau |
|---|---|---|
| STD-13.1 | Struct ordinaire du prélude, un champ **public** `message: String` ; mobile, non `Copy`. | MUST |
| STD-13.2 | `Error::new(s)` **copie** `s` dans un `String` possédé. | MUST |
| STD-13.3 | Aucun trait `Display`/`Debug` utilisateur ; l'affichage de `Error` est **intégré au runtime**. Aucun `impl` utilisateur sur `Error`. | MUST |
| STD-13.4 | Aucune source, aucun code, aucune catégorie, aucune chaîne de causalité, aucun `From` (pas de traits). | MUST |
| STD-13.5 | `Error` n'est pas comparable (`==` interdit) ; comparer `e.message.as_str()`. | MUST |

### 13.1 Affichage intégré

| Contexte | Sortie | Statut |
|---|---|---|
| `main() -> Result<(), Error>` rendant `Err(e)` | `error: <message>` sur **stderr**, code `1` | MUST |
| `?` dans `main` | idem | MUST |
| `io::println(e.message.as_str())` | texte + `\n` sur stdout | MUST |
| `Ok(())` dans `main` | rien, code `0` | MUST |

| ID | Sémantique | Niveau |
|---|---|---|
| STD-13.6 | Le préfixe est **exactement** `error: ` (minuscules, deux-points, une espace), suivi du message et d'un `\n`. Aucune trace de pile, aucun numéro de ligne. | MUST |

```rv
// POSITIF
use std::fs;
use std::io;

fn main() -> Result<(), Error> {
    let s = fs::read_to_string("absent.txt")?;   // affiche : error: <message>, code 1
    io::println(s.as_str());
    return Ok(());
}

fn g() {
    let e = Error::new("boum");
    io::eprintln(e.message.as_str());
}
```

```rv
// NÉGATIF — E-TYP-TRY (main ne rend pas Result)
fn main() { let s = fs::read_to_string("x")?; }
```

## 14. Ce qui n'existe pas (liste fermée)

| Catégorie | Absent | Code |
|---|---|---|
| Formatage | `format!`, `println!`, `{}`, `Debug`, `Display` | `E-PARSE-UNEXPECTED` (macro) / `E-RES-UNKNOWN` |
| Collections | `HashMap`, `HashSet`, `BTreeMap`, itérateurs | `E-RES-UNKNOWN` |
| E/S riches | `File`, `BufReader`, `stdin()`, `stdout()`, `flush` | `E-RES-UNKNOWN` |
| Processus | `Command`, `spawn`, `current_dir`, `env::set_var` | `E-RES-UNKNOWN` |
| Temps | `Instant`, `Duration`, `SystemTime`, `sleep` | `E-RES-UNKNOWN` |
| Threads | `thread::spawn`, `Mutex`, `Arc`, atomiques | `E-RES-UNKNOWN` / `E-UNSUPPORTED-EFFECT` |
| Réseau | `TcpStream`, `UdpSocket` | `E-RES-UNKNOWN` / `E-UNSUPPORTED-EFFECT` |
| Math étendue | `asin`, `acos`, `atan`, `atan2`, `log2`, `hypot`, `PI` | `E-RES-UNKNOWN` |
| Conversions | `From`/`Into`, `to_string` universel, `parse` générique | `E-RES-UNKNOWN` |
| Panic | `catch_unwind`, `Result` de panic, `assert!` avec message | `E-RES-UNKNOWN` |
| Comparaison | `PartialEq`/`Ord` sur agrégats | `E-TYP-EQ` / `E-TYP-CMP` |
| Appel de module | `io.println(...)` (point) | `E-RES-UNKNOWN` |

## 15. Encodage, tampons, atomicité — récapitulatif

| Aspect | Règle |
|---|---|
| Encodage texte | UTF-8 strict partout ; aucune normalisation Unicode ; aucun BOM écrit |
| Fin de ligne écrite | `\n` (`0x0A`) seulement, jamais `\r\n` |
| Lecture fichier | UTF-8 strict ; `\r\n` conservé tel quel dans le `String` |
| Écriture fichier | `write_string` non atomique, non durable, sans répertoire intermédiaire |
| stdout/stderr | flux indépendants, ordre relatif non spécifié, aucun `flush` |
| stdin | ligne seulement, `\n`/`\r\n` retiré, EOF = `Err` |
| Nombres | `to_string` déterministe (Display Rust) ; `pow`/`powf` non garantis |
| Erreurs | `message` non stable, aucun code |
| Allocations | toute opération texte rend un `String` possédé (une allocation) |

## 16. Questions ouvertes

- **Q-STD-1 (résolue par le Lead)** — Forme normative `io::println` ; `io.println` non supporté (`E-RES-UNKNOWN`) et listé dans [`unsupported.md`](unsupported.md) ; les exemples ont été corrigés.
- **Q-STD-2 (bloquante)** — `fs::exists` rend `bool` et avale les erreurs (STD-3.5) : impossible de distinguer « absent » de « permission refusée ». Le design impose `bool`. Recommandation : documenter la limitation et interdire son usage pour une décision de sécurité ; ne pas ajouter `try_exists` sans décision.
- **Q-STD-3** — `env::args` panic sur un argument non UTF-8. Alternative : `args_os()` (nouveau type) — non retenu. À ratifier.
- **Q-STD-4 (bloquante pour la conformance)** — `to_string` flottant et `parse_f64` doivent être **identiques** entre implémentation de référence et oracle indépendant. La règle « plus courte représentation qui round-trip » (STD-12.2) doit être validée sur un corpus de valeurs limites (`2^53`, dénormaux, `1e308`, `-0.0`, `NaN`) avant d'être dite stable. `pow`/`powf` restent **hors** de la conformance bit-exacte.
- **Q-STD-5 (tranchée)** — `Error` reste `{ message: String }` : **aucun code/kind**. Limite documentée et **non bloquante** : la classification d'erreur n'est pas stable, un programme ne peut pas distinguer programmatiquement « absent » de « permission refusée » ni « format invalide » de « dépassement ». Ne jamais s'appuyer sur `message`.
- **Q-STD-6 (résolue par le Lead)** — `std::math` fournit `sin`, `cos`, `tan`, `exp`, `ln`, `log10` en plus de `abs/sqrt/floor/ceil/round/pow/min/max`. Précision **non garantie** (libm) : ces fonctions sont exclues des tests bit-exacts ; les tests de conformance utilisent des tolérances ou des valeurs exactes.
- **Q-STD-7** — `Vec<T>::clone` conditionné à `T: Copy` (STD-10.4) est une règle structurelle spéciale, non exprimable sans bornes. À ratifier.
- **Q-STD-8** — `assert(bool)` sans message (STD-8.6) : la liste du Lead ne prévoit pas de second argument. Un message est souhaitable pour les tests ; à ajouter ou non.
- **Q-STD-9** — `String::new()` et `String::from()` sont des ajouts minimaux nécessaires (construction) absents de la liste exacte du Lead ; `string::from` les couvre fonctionnellement. À ratifier.
- **Q-STD-10** — Les opérations texte possédées coûtent une allocation par appel (`trim`, `lines`, `replace`). Aucun chiffre n'est publié ; une mesure réelle est nécessaire avant toute affirmation de performance.
