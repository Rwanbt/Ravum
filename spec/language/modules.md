# Ravum Core 1.0 — Modules, packages et dépendances

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.1, §3.5.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR**. Aucune implémentation n'est revendiquée (`NOT_IMPLEMENTED`).
> La grammaire des items `mod`/`use` est dans [`grammar.md`](grammar.md) §4 ; la visibilité interagit avec [`ownership.md`](ownership.md) OWN-7.3 et [`diagnostics.md`](diagnostics.md) §3.

## 1. Arbre de modules

| ID | Règle | Niveau |
|---|---|---|
| MOD-1.1 | Un **crate** est l'arbre de modules dont la racine est `src/main.rv` (crate binaire) ou `src/lib.rv` (crate bibliothèque). Il n'y a pas de crate anonyme : tout fichier compilé appartient à un package et à un crate racine. | MUST |
| MOD-1.2 | Un module se déclare soit en **fichier** (`mod foo;`), soit **inline** (`mod foo { ... }`). Les deux formes sont exclusives pour un même nom. | MUST |
| MOD-1.3 | `mod foo;` résout, dans l'ordre : `src/foo.rv`, puis `src/foo/mod.rv` (répertoire du module courant, §2). Si **les deux** existent, `E-MOD-AMBIGUOUS`. Si aucun n'existe, `E-MOD-MISSING`. | MUST |
| MOD-1.4 | Un module `mod foo { ... }` peut contenir des items et des `mod` imbriqués ; un module fichier peut contenir des items et des `mod` (résolus relativement à son propre répertoire). | MUST |
| MOD-1.5 | L'arbre des modules est un **arbre** : un module n'est déclaré qu'une fois. Deux `mod foo;` au même endroit (ou dans deux modules distincts avec le même chemin de fichier) sont `E-RES-DUPLICATE`. Un cycle de modules est donc impossible par construction ; un fichier inclus deux fois est `E-RES-DUPLICATE`. | MUST |
| MOD-1.6 | Le nom d'un module est un identifiant ASCII (§ [`lexical.md`](lexical.md) §4) ; il **MUST** correspondre au nom de fichier/répertoire résolu (à l'extension près). Un `mod foo;` qui résout `src/bar.rv` est impossible par construction (la résolution est déterministe) ; un module inline peut avoir n'importe quel nom ASCII. | MUST |
| MOD-1.7 | Un `main.rv` **MUST** déclarer `fn main()` (voir MOD-4.5). Un `lib.rv` n'a pas de `main` obligatoire (il peut en avoir un, ignoré, voir Q-MOD-7). | MUST |
| MOD-1.8 | `src/main.rv` **et** `src/lib.rv` ne peuvent **pas** coexister : la présence des deux est `E-MOD-BOTH-ROOTS` (Q-MOD-1). Un package est binaire **ou** bibliothèque. L'import d'une bibliothèque par le binaire du même package reste `UNSUPPORTED` (aucune auto-dépendance). | MUST |

```text
hello/
  ravum.toml
  src/
    main.rv        // crate binaire, racine
    lib.rv         // crate bibliothèque, racine (indépendant)
    util.rv        // module de... voir MOD-2.1 (ambigu)
    geo/
      mod.rv
      point.rv
```

```rv
// POSITIF — src/main.rv
mod geo;                 // résout src/geo/mod.rv
mod util;                // résout src/util.rv

fn main() {
    let p = geo::point::new(1.0, 2.0);
    io::println("ok");
}
```

```rv
// NÉGATIF — E-MOD-MISSING
mod absent;              // ni src/absent.rv ni src/absent/mod.rv

// NÉGATIF — E-MOD-AMBIGUOUS
mod util;                // src/util.rv ET src/util/mod.rv existent
```

## 2. Résolution des chemins de modules

| ID | Règle | Niveau |
|---|---|---|
| MOD-2.1 | Pour un module **fichier** `m` situé dans le fichier `F`, les `mod x;` qu'il contient se résolvent dans le répertoire `dir(F)` : `dir(F)/x.rv` ou `dir(F)/x/mod.rv`. Pour la racine, `dir` est `src/`. | MUST |
| MOD-2.2 | Pour un module **inline** `m` déclaré dans le fichier `F`, les `mod x;` qu'il contient se résolvent dans `dir(F)/m/x.rv` ou `dir(F)/m/x/mod.rv` (le nom du module inline ajoute un segment de répertoire). | MUST |
| MOD-2.3 | Un chemin de module ne peut **jamais** sortir de la racine du package : ni `..`, ni chemin absolu, ni lettre de lecteur. Violation : `E-MOD-PATH-ESCAPE`. | MUST |
| MOD-2.4 | Un fichier de module **MUST** se trouver sous `src/` du package ; un `mod` pointant hors de `src/` est `E-MOD-PATH-ESCAPE`. | MUST |
| MOD-2.5 | Les liens symboliques sont **refusés** pour la résolution de modules comme pour les dépendances (`E-MOD-PATH`), afin que la résolution soit reproductible et sans échappement. | MUST |
| MOD-2.6 | La résolution est **déterministe** et ne dépend ni de l'ordre du système de fichiers, ni de la casse (les noms sont sensibles à la casse **et** doivent correspondre exactement). Sur un système de fichiers insensible à la casse, une collision de deux modules ne différant que par la casse est `E-RES-DUPLICATE`. | MUST |
| MOD-2.7 | Un module déclaré mais dont le fichier n'existe pas est une erreur, pas un module vide. | MUST |

```rv
// POSITIF — src/geo/mod.rv contient :
// mod point;            -> src/geo/point.rv
// mod forme { mod svg; } -> src/geo/forme/svg.rv

// NÉGATIF — E-MOD-PATH
mod interdit;            // résoudrait ../interdit.rv
```

## 3. `use`, `crate`, `super`, `self`

| ID | Règle | Niveau |
|---|---|---|
| MOD-3.1 | `use chemin;` importe **un** nom (item, module, type, fonction, constante, variante de type) dans le module courant. Il n'importe jamais « tout un module ». | MUST |
| MOD-3.2 | Formes admises : `use crate::a::B;`, `use super::x;`, `use self::y;`, `use std::io;`, `use dep::helper;`. Un chemin relatif nu (`use foo::Bar;`) est interprété **relativement au module courant** d'abord, puis à la racine du crate (`crate::foo::Bar`) — voir Q-MOD-4 ; en cas de double résolution, `E-RES-CONFLICT`. | MUST |
| MOD-3.3 | `use a::*` (glob), `use a as b` (alias), `pub use` (ré-export) sont refusés : `E-UNSUPPORTED-TOKEN`, `E-UNSUPPORTED-TOKEN`, `E-UNSUPPORTED-TOKEN`. | MUST |
| MOD-3.4 | Un import inutilisé est `W-RES-UNUSED-IMPORT` (avertissement, jamais une erreur). | SHOULD |
| MOD-3.5 | Les variantes d'enum **utilisateur** ne sont jamais importables individuellement (`use Shape::Circle;` → `E-RES-UNKNOWN`). Les quatre constructeurs du prélude (`Some`, `None`, `Ok`, `Err`) sont disponibles **sans import** et acceptés qualifiés ou non ([`types.md`](types.md) TYP-10.2). | MUST |
| MOD-3.6 | `crate` désigne la racine du crate courant ; `super` le module parent (erreur `E-RES-UNKNOWN` à la racine) ; `self` le module courant. | MUST |
| MOD-3.7 | Deux `use` important le **même nom** dans le même module sont `E-RES-DUPLICATE`, même si l'un vient de `crate` et l'autre d'une dépendance. Un nom importé ne peut pas entrer en collision avec un item déclaré dans le module (`E-RES-DUPLICATE`). | MUST |
| MOD-3.8 | Un import n'a **aucun effet d'exécution** : il ne déclenche aucune initialisation, aucun code. Importer un module ne « charge » rien. | MUST |
| MOD-3.9 | `use` ne peut pas importer un item **privé** d'un autre module (`E-RES-PRIVATE`), même transitivement. | MUST |

```rv
// POSITIF
use crate::geo::point::Point;
use super::util::helper;
use std::io;
use dep::convert::parse_i64;

fn main() { io::println("ok"); }
```

```rv
// NÉGATIF — E-UNSUPPORTED-TOKEN
use crate::geo::*;
// NÉGATIF — E-UNSUPPORTED-TOKEN
use crate::geo::point::Point as P;
// NÉGATIF — E-UNSUPPORTED-TOKEN
pub use crate::geo::point::Point;
// NÉGATIF — E-RES-UNKNOWN
use Option::Some;
// NÉGATIF — E-RES-DUPLICATE
use crate::a::Thing;
use crate::b::Thing;
```

## 4. Visibilité

| ID | Règle | Niveau |
|---|---|---|
| MOD-4.1 | Tout item, champ et module est **privé par défaut**. | MUST |
| MOD-4.2 | Un item `pub` est visible depuis **tout** le crate (y compris les modules descendants), et depuis les packages dépendants si **tous** les modules du chemin sont eux-mêmes `pub`. | MUST |
| MOD-4.3 | Un item privé est visible dans son module de définition **et dans tous ses modules descendants** (règle « privé = visible dans le sous-arbre »). Un module parent **ne voit pas** les items privés de ses enfants. | MUST |
| MOD-4.4 | Un champ privé n'est accessible que dans le module de définition de la struct et ses descendants ; l'accès hors de là est `E-RES-PRIVATE` (vérifié **avant** la mutabilité, [`ownership.md`](ownership.md) OWN-7.3). | MUST |
| MOD-4.5 | `fn main` **MUST** être déclarée dans la racine d'un crate binaire, sans `pub`, sans paramètre, sans générique, et avoir le type `()` ou `Result<(), Error>` (design §3.8). Toute autre forme est `E-TYP-MAIN`. `main` manquante est `E-TYP-MAIN-MISSING` ; deux `main` sont `E-TYP-MAIN-DUPLICATE`. | MUST |
| MOD-4.6 | La visibilité d'une **variante** d'enum est celle de l'enum ; `pub` sur une variante est accepté sans effet (G-4.2). | MUST |
| MOD-4.7 | Une méthode d'`impl` suit la visibilité de sa déclaration (`pub fn`), indépendamment de la visibilité du type. Une méthode privée n'est appelable que dans le sous-arbre du module de l'`impl`. | MUST |
| MOD-4.8 | `const` et `struct`/`enum` suivent les mêmes règles que `fn`. Un type privé ne peut pas apparaître dans la signature publique d'un item `pub` sans que l'usage soit refusé chez le dépendant (`E-RES-PRIVATE`, « type privé dans une API publique »). | MUST |
| MOD-4.9 | Il n'existe **aucune** visibilité `pub(crate)`, `pub(super)`, `pub(in ...)` : `pub` est binaire. Toute autre forme est `E-PARSE-EXPECTED`. | MUST |
| MOD-4.10 | `test fn` suit les règles de `fn` : elle est privée par défaut, n'a aucun paramètre, et son retour est `()` ou `Result<(), Error>` ([`grammar.md`](grammar.md) G-3.6). Elle n'est pas appelable depuis `main`. | MUST |

```rv
// POSITIF
mod geo {
    pub struct Point { pub x: f64, y: f64 }     // x public, y privé
    impl Point {
        pub fn new(x: f64, y: f64) -> Point { return Point { x: x, y: y }; }
        pub fn y(&self) -> f64 { return self.y; }   // accesseur du champ privé
    }
}

fn main() {
    let p = geo::Point::new(1.0, 2.0);
    let a = p.x;        // OK : champ pub
    let b = p.y();      // OK : accesseur
}
```

```rv
// NÉGATIF — E-RES-PRIVATE (champ privé)
fn main() { let p = geo::Point::new(1.0, 2.0); let b = p.y; }
// NÉGATIF — E-RES-PRIVATE (item privé d'un module enfant)
mod a { fn cache() { } }
fn main() { a::cache(); }
// NÉGATIF — E-TYP-MAIN (type de retour invalide)
fn main() -> i32 { return 0; }
// NÉGATIF — E-TYP-MAIN-MISSING
fn demarrage() { }
```

## 5. Package : `ravum.toml`

```toml
[package]
name = "hello"          # [a-z][a-z0-9_-]*
version = "0.1.0"       # semver minimal : MAJOR.MINOR.PATCH, chiffres
edition = "core-1.0"

[dependencies]
util = { path = "../util" }
```

| ID | Règle | Niveau |
|---|---|---|
| MOD-5.1 | Un package est un répertoire contenant `ravum.toml` à sa racine. Un `ravum.toml` illisible ou TOML invalide est `E-MOD-MANIFEST`. | MUST |
| MOD-5.2 | `[package]` exige `name`, `version`, `edition`. `name` vérifie `[a-z][a-z0-9_-]*` (`E-MOD-MANIFEST` sinon) ; `version` vérifie `MAJOR.MINOR.PATCH` avec des entiers décimaux sans zéro de tête (`E-MOD-MANIFEST`) ; `edition` **MUST** valoir `core-1.0` (`E-UNSUPPORTED-EDITION`). | MUST |
| MOD-5.3 | Toute clé inconnue dans `ravum.toml` est `E-MOD-MANIFEST` (manifeste strict : pas de clé « ignorée »). Aucune section `[build]`, `[scripts]`, `[features]`, `[profile]` n'existe. | MUST |
| MOD-5.4 | `[dependencies]` est optionnel. Chaque entrée est `nom = { path = "..." }`. Aucune autre forme (`version = "..."` seule, `git = ...`, `registry = ...`, table vide) n'est acceptée : `E-MOD-DEPENDENCY`. | MUST |
| MOD-5.5 | Le nom d'une dépendance (clé) **MUST** être égal à `package.name` du package cible : `E-MOD-DEPENDENCY`. Deux clés identiques sont impossibles (TOML), deux dépendances résolvant le même répertoire sont `E-MOD-DEPENDENCY`. | MUST |
| MOD-5.6 | `path` est **relatif** au répertoire du manifeste, peut contenir `..` (ex. `../util`) sans dépasser la profondeur **8**, ne peut pas être absolu, ne peut pas traverser un lien symbolique, et **MUST** désigner un répertoire contenant un `ravum.toml` valide. Violations : `E-MOD-DEPENDENCY` (forme), `E-MOD-PATH-ESCAPE` (absolu, profondeur), `E-MOD-PATH` (symlink), `E-MOD-RESERVED-NAME` (nom réservé). | MUST |
| MOD-5.7 | Une dépendance **MUST** exposer `src/lib.rv` (une dépendance binaire n'est pas importable) : `E-MOD-DEPENDENCY`. | MUST |
| MOD-5.8 | Le graphe de dépendances **MUST** être acyclique : `E-MOD-CYCLE` avec le cycle affiché. | MUST |
| MOD-5.9 | Un package **MUST NOT** dépendre de lui-même (même par un chemin détourné) : `E-MOD-CYCLE`. | MUST |
| MOD-5.10 | Il n'y a **aucun** script de build, aucun `build.rs`, aucun `[build]`, aucune exécution de code à la résolution, à l'import ou à la compilation d'une dépendance : les dépendances sont **du code source `.rv` uniquement**. Toute tentative est `E-MOD-BUILD-SCRIPT`. | MUST |
| MOD-5.11 | Il n'y a **aucun** registre public, aucune dépendance distante, aucun téléchargement : toutes les dépendances sont locales (§ design §5). Une dépendance non locale est `E-MOD-DEPENDENCY`. | MUST |
| MOD-5.12 | Le nom de package n'a pas à correspondre au nom du répertoire ; seule la clé de dépendance doit correspondre au `package.name` de la cible. | MUST |
| MOD-5.13 | Un chemin de dépendance ou un nom de répertoire de package **MUST NOT** utiliser un nom réservé Windows (`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`, insensible à la casse, avec ou sans extension) : `E-MOD-RESERVED-NAME`, sur **toutes** les plateformes (refus déterministe). | MUST |

```toml
# POSITIF
[package]
name = "app"
version = "0.2.0"
edition = "core-1.0"

[dependencies]
util = { path = "../util" }
```

```toml
# NÉGATIF — E-MOD-BUILD-SCRIPT ([build] interdit)
[package]
name = "app"
version = "0.2.0"
edition = "core-1.0"

[build]
script = "build.sh"
```

```toml
# NÉGATIF — E-MOD-DEPENDENCY (version seule, pas de path)
[dependencies]
util = "1.0.0"

# NÉGATIF — E-MOD-MANIFEST (clé inconnue)
[package]
name = "app"
version = "0.2.0"
edition = "core-1.0"
license = "MIT"        # clé non supportée en Core 1.0
```

## 6. Import d'une dépendance

| ID | Règle | Niveau |
|---|---|---|
| MOD-6.1 | Une dépendance `util` s'importe par `use util::chemin::Item;`. La racine de `util` est son `src/lib.rv` ; `util::x` désigne l'item `x` de la racine. | MUST |
| MOD-6.2 | Seuls les items `pub` (et les modules `pub` du chemin) d'une dépendance sont accessibles (`E-RES-PRIVATE`). | MUST |
| MOD-6.3 | Importer une dépendance non déclarée dans `ravum.toml` est `E-RES-UNKNOWN`. | MUST |
| MOD-6.4 | Les effets d'une dépendance ne « fuient » pas : une fonction de dépendance a son propre ensemble d'effets, et l'appeler fait entrer ces effets dans l'appelant (`E-EFF-UNDECLARED` si non déclarés, [`effects.md`](effects.md) EFF-2.1). | MUST |
| MOD-6.5 | Aucune macro, aucun attribut, aucune extension de compilateur ne peut être fournie par une dépendance : il n'y a pas de plugins. | MUST |
| MOD-6.6 | Les dépendances sont compilées **à partir de leur source** dans le même programme ; il n'existe pas de bibliothèque précompilée, pas d'ABI entre packages (voir [`abi.md`](abi.md)). | MUST |
| MOD-6.7 | La forme `module.fonction(...)` (avec un **point**) n'existe **pas** : `io.println("x")` est `E-RES-UNKNOWN` (aucune méthode `println` sur un module). La forme normative est `io::println("x")` ; `io.println` est listé `unsupported` ([`unsupported.md`](unsupported.md)). | MUST |

```rv
// POSITIF — app/src/main.rv
use util::convert::parse_i64;

fn main() {
    let n = parseIntOrZero("42");
    io::println("ok");
}
fn parseIntOrZero(s: &str) -> i64 {
    match parse_i64(s) { Result::Ok(v) => v, Result::Err(e) => 0 }
}
```

```rv
// NÉGATIF — E-RES-UNKNOWN
use autre::helper;        // `autre` absent de [dependencies]
// NÉGATIF — E-RES-PRIVATE (item privé de la dépendance)
use util::cache::interne;
```

## 7. `ravum.lock` : résolution déterministe et digest

Format (candidate, propriétaire Ravum — pas un format externe) :

```toml
# ravum.lock — généré, ne pas éditer
lockfile_version = 1

[[package]]
name = "util"
version = "0.1.0"
digest = "sha256:9f2c...e1"

[[package]]
name = "core"
version = "1.0.0"
digest = "sha256:0000...00"    # package implicitement disponible (voir §7.3)
```

| ID | Règle | Niveau |
|---|---|---|
| MOD-7.1 | `ravum.lock` contient une entrée par package **résolu**, triées par `name` croissant (ordre lexicographique ASCII), puis `version`. La sortie est **byte-identique** pour une même entrée. | MUST |
| MOD-7.2 | `digest` est le SHA-256 du **contenu combiné** du package : tous les fichiers `.rv` sous `src/` triés par chemin relatif, plus `ravum.toml`, chacun préfixé par son chemin relatif et sa longueur (encodage défini, voir Q-MOD-6). Une modification d'un `.rv` ou du manifeste change le digest. | MUST |
| MOD-7.3 | Le prélude `std`/`core` est **implicite** : il n'apparaît pas dans `[dependencies]` et n'est pas requis dans le lockfile (voir Q-MOD-5). | MUST |
| MOD-7.4 | `check --json` expose `dependencies_digest` : digest agrégé déterministe de toutes les dépendances résolues. Une modification d'une dépendance locale est donc **détectable** (design §3.5). | MUST |
| MOD-7.5 | Un `ravum.lock` absent est créé par `build`/`check`. Un lockfile **périmé** (digest différent) ne produit **aucun diagnostic** (liste d'avertissements fermée) : `check --json` expose `dependencies_digest` et `lockfile_stale: true`. Ce n'est pas une erreur de compilation. | MUST |
| MOD-7.6 | Le lockfile ne contient **aucune** URL, aucun checksum de registre, aucune signature : uniquement nom, version, digest local. | MUST |
| MOD-7.7 | Un lockfile avec `lockfile_version` inconnu est `E-MOD-LOCK` (fail-closed, jamais interprété partiellement). | MUST |
| MOD-7.8 | La résolution ne fait **aucun** choix de version : chemins locaux uniquement, donc aucune contrainte à satisfaire, aucun backtracking, aucun solveur. | MUST |

```rv
// POSITIF — deux exécutions de `build` produisent le même ravum.lock
$ ravum-lang build
$ sha256sum ravum.lock
# identique

// NÉGATIF — E-MOD-LOCK
lockfile_version = 99
```

## 8. Compatibilité semver minimale

| ID | Règle | Niveau |
|---|---|---|
| MOD-8.1 | Une version est `MAJOR.MINOR.PATCH`, trois entiers décimaux, sans pré-release ni build metadata (`E-MOD-MANIFEST` sinon). | MUST |
| MOD-8.2 | `edition = "core-1.0"` est la seule édition acceptée. Une édition inconnue est `E-UNSUPPORTED-EDITION` (fail-closed). | MUST |
| MOD-8.3 | Si une dépendance déclare une `version` dans son `ravum.toml` et que le dépendant la référence par chemin, **aucune** contrainte de version n'est appliquée : le chemin fait foi. La version sert à l'identification et au lockfile. | MUST |
| MOD-8.4 | Il n'existe **aucune** plage de versions (`^`, `~`, `>=`, `*`) ni résolution de contraintes en Core 1.0 : `E-MOD-DEPENDENCY`. | MUST |
| MOD-8.5 | Deux dépendances différentes (chemins différents) portant le même `name` sont `E-MOD-DEPENDENCY`, même si leurs versions diffèrent : pas de résolution multi-version. | MUST |
| MOD-8.6 | La compatibilité ascendante des packages est **déclarative** en Core 1.0 : rien n'est vérifié automatiquement au-delà de l'édition et de la compilation effective. Voir [`compatibility.md`](compatibility.md) §3. | MUST |

### 8.3 Composition du package implicite `std`

`std` est un package fourni par le compilateur : non déclarable, non verrouillé, non remplaçable, non versionné par l'utilisateur. Modules : `io`, `fs`, `env`, `process`, `math`, `string`, `panic`. Le **prélude** (disponible sans `use`) contient : les types primitifs, `String`, `Vec<T>`, `Option<T>`, `Result<T, E>`, `Error`, les quatre constructeurs `Some`/`None`/`Ok`/`Err`, et `assert`/`assert_eq`/`panic`/`drop`. `std::panic` ré-expose `assert`/`assert_eq`/`panic`. Il n'existe **pas** de `std::convert` : les conversions texte sont dans `std::string` et les méthodes `to_string` ([`stdlib.md`](stdlib.md)).

## 9. Récapitulatif des codes

| Code | Sens |
|---|---|
| `E-MOD-MISSING` | fichier de module absent |
| `E-MOD-AMBIGUOUS` | `foo.rv` **et** `foo/mod.rv` |
| `E-MOD-PATH` | module hors de `src/` ou chemin avec `..` |
| `E-MOD-BOTH-ROOTS` | `src/main.rv` **et** `src/lib.rv` présents |
| `E-RES-DUPLICATE` | module/item/import déclaré deux fois |
| `E-RES-UNKNOWN` | `use` d'une variante, `super` à la racine |
| `E-RES-PRIVATE` | item/champ privé accédé hors de son sous-arbre |
| `E-RES-UNKNOWN` | dépendance non déclarée |
| `E-TYP-MAIN-MISSING`, `E-TYP-MAIN-DUPLICATE` | `main` absente ou multiple |
| `E-MOD-MANIFEST`, `E-MOD-MANIFEST` | `ravum.toml` invalide / clé inconnue |
| `E-MOD-MANIFEST`, `E-MOD-MANIFEST`, `E-UNSUPPORTED-EDITION` | nom/version/édition invalides |
| `E-MOD-DEPENDENCY`, `E-MOD-DEPENDENCY`, `E-MOD-DEPENDENCY`, `E-MOD-DEPENDENCY` | dépendance malformée |
| `E-MOD-PATH` | lien symbolique ou forme de chemin invalide |
| `E-MOD-PATH-ESCAPE` | chemin absolu, `..` interdit, profondeur > 8 |
| `E-MOD-RESERVED-NAME` | nom réservé Windows |
| `E-MOD-BUILD-SCRIPT` | build script refusé |
| `E-MOD-CYCLE` | cycle de dépendances |
| `E-MOD-LOCK` | lockfile inconnu |
| `E-UNSUPPORTED-TOKEN`, `-ALIAS`, `-REEXPORT`, `-SELF-LIB` | formes d'import non supportées |
| `W-RES-UNUSED-IMPORT` | seul avertissement de ce module |

## 10. Questions ouvertes

- **Q-MOD-1 (résolue par le Lead)** — Les deux racines `main.rv` + `lib.rv` sont **refusées** (`E-MOD-BOTH-ROOTS`) : un package est binaire **ou** bibliothèque. L'import main→lib reste `UNSUPPORTED` ; la migration consiste à créer deux packages ou une dépendance locale. Limite documentée.
- **Q-MOD-2 (résolue par le Lead)** — `..` est **autorisé** dans les chemins de dépendances, relativement au manifeste, jamais absolu, jamais via symlink, profondeur bornée à 8 (MOD-5.6). Les modules restent confinés à `src/` sans `..` (MOD-2.3).
- **Q-MOD-3** — MOD-7.5 : un lockfile périmé n'est qu'un avertissement. Le design dit « détectée », pas « erreur ». Faut-il un mode strict (`--locked`, absent du design) ? Recommandation : non en Core 1.0, mais exposer le digest permet à un CI de comparer.
- **Q-MOD-4** — MOD-3.2 : la résolution d'un `use` relatif nu (module courant puis racine) est ambiguë en cas de collision. Ce document émet `E-RES-CONFLICT`. Alternative plus simple : exiger `crate::`/`super::`/`self::` pour tout chemin local (aucun chemin nu). Recommandation : exiger les préfixes, plus prévisible pour les agents.
- **Q-MOD-5** — MOD-7.3 : le prélude `std` n'est pas une dépendance déclarée. Un lockfile listant `core 1.0.0` (comme dans l'exemple) n'est pas nécessaire ; l'exemple est illustratif. Confirmer que le prélude est **hors** lockfile.
- **Q-MOD-6** — MOD-7.2 : l'encodage exact du digest (préfixes de chemin, séparateurs, longueur, normalisation des fins de ligne) doit être figé pour que deux implémentations produisent le même hash. Proposition : `path\0len\0bytes` concaténés, triés par path UTF-8, avec `\n` normalisé (jamais CRLF). À ratifier.
- **Q-MOD-7** — Une `fn main` dans `src/lib.rv` : MOD-1.7 dit « ignorée ». Aucun avertissement n'existe (`W-*` fermé) ; trancher entre silence et erreur.
- **Q-MOD-8** — MOD-8.4 : l'absence totale de plages de versions signifie qu'aucun écosystème de packages ne peut évoluer sans casser les chemins. C'est cohérent avec « pas de registre public » (design §5), mais à confirmer pour T11.
