# Ravum Core — conception technique candidate (DRAFT, non ratifiée)

> **Statut : DRAFT / CANDIDATE.** Ce document fixe les décisions *techniques et
> réversibles* nécessaires pour implémenter une chaîne Ravum réelle et testable.
> Il ne ratifie aucun ADR, ne choisit aucune licence et ne publie aucune release.
> Toute règle sémantique y est marquée **candidate** et doit être validée par la
> suite de conformance (`conformance/`) avant d'être présentée comme stable.
> Décision humaine ultérieure requise : `DECISIONS_REQUIRED.md` (T12).

Contexte : branche `dev`, base `5408d9b3a48dab2884e2b3eb1aa89c3c16ce3d96` (2026-11-09).
Le socle existant (orchestration Rust, overlays, daemon, GitWriter) est conservé ;
il devient un service du compilateur Ravum, il n'est pas remplacé.

## 1. Décision M5 (bootstrap) — provisoire, expérimentale

Après comparaison des trois voies sur le corpus gelé `fixtures/m5/` (harness
`tools/m5-bench`, résultats `evidence/m5-*.json`) :

| Voie | Description | Verdict provisoire |
|---|---|---|
| A | Rust + outillage Ravum | référence ; ne satisfait pas « un langage Ravum » |
| **B** | frontend Ravum propre → faits typés → **Rust généré auditable** → `rustc` CLI | **retenue** (chaîne réelle, sémantique vérifiée avant émission) |
| C | frontend Ravum → IR propre → Cranelift | **expérimentale**, non qualifiée ; interface backend réservée |

Le backend de référence est donc **B** : `rustc` est utilisé comme *générateur de
code machine*, jamais comme vérificateur de sémantique. Interdits : API privées
`rustc_*`, génération qui contourne le checker, « Rust caché » (le code généré est
lisible via `ravum-lang build --emit-rust`). Le checker Ravum est l'autorité :
il refuse avant émission ; le code émis est un sous-ensemble Rust dont la
sémantique est spécifiée ici.

Durée des builds : `rustc` est invoqué directement (pas de projet Cargo généré),
un seul fichier Rust autonome avec prélude inclus. `--emit-rust` expose le résultat.

## 2. Produit et CLI

Binaire produit : **`ravum-lang`** (crate `ravum-lang-cli`). L'outil historique
`ravum` (orchestration Rust) reste inchangé pendant T0 et n'est pas présenté comme
le compilateur du langage. Unification `ravum` prévue à T7 après stabilisation,
avec alias documenté et compatibilité des sous-commandes existantes.

Commandes **réellement implémentées** (une commande non implémentée n'est pas
affichée dans `--help`, ou est listée `UNSUPPORTED` explicitement) :

```
ravum-lang --version
ravum-lang doctor [--json]
ravum-lang init <dir> [--lib]
ravum-lang check [PATH] [--json] [--profile core|pure|rt]
ravum-lang build [PATH] [--release] [--emit-rust FILE] [--emit-ir]
ravum-lang run [PATH] [-- ARGS...]
ravum-lang test [PATH] [--filter SUBSTR] [--json]
ravum-lang fmt [PATH] [--check]
ravum-lang lint [PATH] [--deny CODE]
ravum-lang doc [PATH] [--out DIR]
ravum-lang cache status|clean
ravum-lang lsp            # si T7-LSP livré, sinon UNSUPPORTED explicite
```

Codes de sortie : `0` succès ; `1` erreur de compilation/diagnostic ou test en
échec ; `2` usage/CLI invalide ; `3` outil externe absent (`rustc`) ou
environnement non supporté. Sortie JSON : schéma versionné
`spec/language/diagnostics.md` (`schema_version: 1`), un objet par ligne ou un
tableau selon la commande, jamais de couleur ni de bruit non structuré.

## 3. Surface du langage Core 1.0 (candidate)

Fichier `.rv`, UTF-8 sans BOM, fins de ligne `\n` (les `\r\n` sont acceptés à la
lecture et normalisés par `fmt`). Commentaires `//` et `/* ... */` (non imbriqués).
Identifiants ASCII `[A-Za-z_][A-Za-z0-9_]*` (identifiants Unicode : UNSUPPORTED
en Core 1.0, erreur `E-UNSUPPORTED-IDENT`). Mots-clés réservés :
`fn let mut const struct enum impl use pub mod match if else while loop for in
break continue return true false as test unsafe self Self crate super trait where
extern async move type static effects _`. `ref`, `dyn`, `await` ne sont **pas**
réservés (identifiants valides, manglés par le backend) ; `type` et `static` sont
réservés pour éviter les collisions futures (UNSUPPORTED s'ils sont utilisés).
Les opérateurs d'affectation composée `+= -= *= /= %= &= |= ^= <<= >>=` sont
tous acceptés.

### 3.1 Items

```
mod foo;                       // charge src/foo.rv ou src/foo/mod.rv
mod foo { ... }                // module inline
use foo::bar;                  // import simple (pas de glob ni d'alias en Core 1.0)
pub fn f<T>(x: T) -> T { ... }
fn f(p: T, q: &mut U) -> Result<R, Error> effects(fs, alloc) { ... }
test fn nom_du_test() { ... }  // test unitaire, sans paramètre ; retour () ou
                              // Result<(), Error> (Err = test en échec)
struct Point { pub x: f64, y: f64 }
enum Shape { Circle(f64), Rect { w: f64, h: f64 }, Empty }
impl Point { fn norme(&self) -> f64 { ... } fn new(x: f64) -> Point { ... } }
const N: i32 = 10;
```

- Visibilité : privé par défaut ; `pub` sur items, champs et variantes.
  L'accès inter-module à un item privé est refusé (`E-RES-PRIVATE`).
- `impl` : méthodes inhérentes uniquement (`&self`, `&mut self`, `self`) et
  fonctions associées sans `self`. Pas de `trait` en Core 1.0
  (`E-UNSUPPORTED-TRAIT`) ; les génériques n'ont pas de bornes (`where` interdit).
- `unsafe { ... }` et `extern "C" { ... }` sont supportés dans un cadre étroit et
  explicite (§3.7 bis) ; les pointeurs bruts, `unsafe fn`, `extern "C" fn` avec
  corps, les ABI autres que `"C"` restent refusés (`E-UNSUPPORTED-*`).
- `async`, `move`, `trait`, `where`, `impl Trait` : mots-clés réservés, rejetés
  avec `E-UNSUPPORTED-*` — jamais exécutés par accident du backend.

### 3.2 Types Core 1.0

`i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 isize usize f32 f64 bool char ()`
`String` (UTF-8 possédé), `&T`, `&mut T`, `[T; N]`, `&[T]`, tuples `(T, ...)`,
`Vec<T>`, `Option<T>`, `Result<T, E>`, types nominaux, paramètres génériques
`<T>` (monomorphisation par le backend ; pas de bornes).

`str` n'existe pas comme type nommé en Core 1.0 : les littéraux de texte ont le
type `&'static str` (noté `&text` dans les diagnostics), les paramètres de texte
sont `&str`, et `String::from(s: &str) -> String` possède. Les références ne
peuvent pas être stockées ni retournées (§4.4).

Conversions : aucune conversion implicite entre types numériques. `as` explicite,
sémantique de Rust documentée (entier→entier troncature/extension, entier→flottant
arrondi, flottant→entier saturant) : définie, jamais UB. Deux **coercitions
implicites** seulement : `&[T; N]` → `&[T]` et `&String` → `&str`, aux arguments
et aux récepteurs de méthode. Littéraux entiers : le type est déterminé par le
contexte **immédiat** (initialisation, argument, opérande) ; sans contexte, i32.
Une variable entière non annotée est i32 ; il n'y a **pas** d'unification
rétroactive du type d'une variable depuis ses usages ultérieurs : annoter
`let mut i: u32 = 0;` (limite documentée, `spec/language/types.md`). Un littéral
qui ne tient pas dans le type inféré est une erreur statique
(`E-TYP-LITERAL-RANGE`) ; `-128i8` reste accepté via le moins unaire. Littéraux
flottants défaut `f64`.

### 3.3 Instructions et expressions

```
let x: T = e;  let mut y = e;  let z: T;   // init différée autorisée, analyse d'init
x = e;  x += e;                            // affectation, place mutable uniquement
return e;  break;  continue;
if c { } else if c2 { } else { }           // expression
match v { Pat => e, ... }                  // expression, exhaustif
while c { }  loop { }  for x in 0..n { }   // 0..n exclusif
{ ... }                                    // bloc expression
```

Opérateurs par précédence décroissante : postfixes (`()`, `[]`, `.`, `?`) ;
unaires `- ! & &mut *` ; `as` ; `* / %` ; `+ -` ; `<< >>` ; `&` ; `^` ; `|` ;
comparaisons ; `&&` ; `||` ; `..` (uniquement dans `for`) ; affectation.
`&&`/`||` court-circuitent. Évaluation de gauche à droite, arguments de gauche à
droite. Exception documentée (audit backend F3) : pour `place = valeur`, la
valeur est évaluée avant la place, comme en Rust ; ce n'est observable que si la
place a des effets de bord (`a[f()] = g()`), cas non interdit par le checker.
Débordement entier : **panic contrôlé** dans tous les profils
(`-C overflow-checks=on`), division par zéro et reste par zéro : panic, index
hors bornes : panic, décalage ≥ largeur : panic. Flottants : IEEE-754, NaN
documenté, `0.0/-0.0`, pas de piège. `panic`, `assert`, `assert_eq` sont des
fonctions du prélude (effet `panic`).

### 3.4 Patterns et exhaustivité

Littéraux, `_`, liaison `x`, tuple `(p, q)`, variante `E::V(p)` /
`E::V { champ: p }` (raccourci `E::V { champ }` accepté), `Option::Some(x)`.
Pas de garde `if`, pas de pattern de référence en Core 1.0. `match` doit être
exhaustif sur les enums et `bool` (vérifié par le checker, pas seulement par
rustc) ; les entiers exigent `_`. Un `match` non exhaustif est `E-TYP-NONEXHAUSTIVE`.

### 3.5 Modules, packages

- Un package = répertoire avec `ravum.toml` :
  ```toml
  [package]
  name = "hello"          # [a-z][a-z0-9_-]*
  version = "0.1.0"       # semver minimal
  edition = "core-1.0"

  [dependencies]
  util = { path = "../util" }
  ```
- Racine : `src/main.rv` (binaire) **ou** `src/lib.rv` (bibliothèque) ; les deux
  simultanément sont refusés en Core 1.0 (`E-MOD-BOTH-ROOTS`) — l'import
  main→lib d'un même package reste UNSUPPORTED (limite documentée).
  `mod foo;` résout `src/foo.rv` puis `src/foo/mod.rv` ; erreur si les deux.
- `use` : `use crate::foo::Bar;`, `use super::x;`, `use std::io;`, `use util::helper;`
  (dépendance locale). Pas de glob, pas d'alias, pas de ré-export `pub use` en
  Core 1.0 (UNSUPPORTED explicite). Les modules sont confinés à `src/` ; les
  dépendances locales peuvent utiliser `..` (chemin relatif au manifeste) mais
  jamais sortir par symlink et jamais un chemin absolu.
- `ravum.lock` : résolution déterministe (tri par nom), hash SHA-256 du contenu de
  chaque package dépendant (tous les `.rv` + `ravum.toml`). Une modification de
  dépendance locale est détectée (`check --json` expose `dependencies_digest`).
- Une dépendance est du **code source `.rv`** ; aucun build script, aucune
  exécution de code à l'import. Chemins validés : composants contrôlés, pas de
  chemin absolu, symlinks/reparse points refusés, profondeur et taille bornées.

### 3.6 Effets (capacités)

Groupes : `io fs net alloc panic unsafe ffi async rt nondet`. Une fonction peut
déclarer `effects(a, b)` : ensemble **majorant** de ses effets. Le checker
infère les effets du corps et de la fermeture transitive des appels ; une
déclaration qui n'inclut pas un effet inféré est une erreur (`E-EFF-UNDECLARED`).
Sans déclaration, l'ensemble inféré est public. Profils :
- `core` (défaut) : aucune restriction ;
- `pure` : `main` (ou la fonction testée) ne doit atteindre **aucun** effet de
  l'ensemble interdit `io fs net alloc panic unsafe ffi async rt nondet` ; toute
  atteinte est `E-EFF-PROFILE`. Limite assumée : les traps intrinsèques
  (débordement, division par zéro, index) ne sont pas des effets et peuvent donc
  encore arrêter un programme `pure`.
- `rt` : **expérimental** — interdit `io fs net alloc panic unsafe ffi async rt`
  dans le corps, vérifié transitivement ; ne certifie **pas** le hard-RT.
`unsafe`, `ffi`, `net`, `async` : `unsafe` et `ffi` sont **inférés** par les
blocs `unsafe` et les appels `extern` (jamais déclarés silencieusement) ;
`net` et `async` ne sont pas implémentés, les déclarer reste
`E-UNSUPPORTED-EFFECT`. `alloc` est approché à la granularité fonction :
toute construction `String`/`Vec`/appel de fonction allouante marque `alloc`
(il n'y a pas de `Box` en Core 1.0).

### 3.7 Propriété, emprunts, drop (sous-ensemble conservateur)

Types `Copy` : primitives, `char`, `bool`, `()`, références, tuples/arrays de
`Copy`, structs/enums dont tous les champs sont `Copy`. Tout le reste est
**mobile** (`String`, `Vec`, structs/enums non-Copy).

Règles vérifiées (et non déléguées à rustc) :
1. **Initialisation** : lecture d'une variable non initialisée sur un chemin
   (`E-OWN-UNINIT`) ; `let x: T;` doit être affectée avant tout usage sur tous
   les chemins.
2. **Move** : un mobile utilisé après déplacement est refusé (`E-OWN-USE-AFTER-MOVE`) ;
   double move refusé (`E-OWN-DOUBLE-MOVE`). Le **move partiel** est autorisé et
   suivi par champ : déplacer `s.champ` marque le champ comme déplacé ; l'usage
   ultérieur du struct entier est refusé (`E-OWN-PARTIAL-MOVE`), mais déplacer
   d'autres champs ou utiliser les champs encore intacts reste permis. C'est ce
   qui rend possible `fn into_titre(self) -> String { self.titre }` et le retour
   d'un champ possédé. Il n'y a pas de pattern de struct en Core 1.0.
3. **Borrows** : `&x`/`&mut x` créent des prêts qui vivent au plus jusqu'à la fin
   de l'instruction (ou de l'appel) englobante. Pendant un prêt exclusif, aucune
   lecture/écriture/autre prêt du même lieu (`E-OWN-BORROW-CONFLICT`) ; pendant
   des prêts partagés, aucune écriture ni prêt exclusif. `f(&mut x, &mut x)` et
   `f(&x, &mut x)` sont refusés.
4. **Non-échappement** : une référence ne peut pas être stockée dans une variable,
   un champ, un élément de collection, ni retournée par une fonction
   (`E-OWN-REF-ESCAPE`) — sauf `&'static str` littéral, qui est `Copy`. Les
   paramètres `&T`/`&mut T` sont utilisables dans tout le corps (prêt lié à
   l'appel) ; ils ne peuvent pas être retournés ni stockés.
5. **Drop** : les valeurs sont détruites en ordre inverse de déclaration à la fin
   du bloc ; `drop(x)` du prélude force la destruction ; un mobile déplacé n'est
   pas détruit. Pas de `Drop` utilisateur ni de destructeur personnalisé en Core 1.0
   (`E-UNSUPPORTED-DROP-IMPL`).
6. **unsafe/FFI** : les blocs `unsafe { ... }` sont des blocs ordinaires pour
   l'ownership ; ils ajoutent l'effet `unsafe` et autorisent l'appel de
   fonctions `extern`. Les pointeurs bruts restent absents : un appel `extern`
   ne peut passer que des primitives FFI-sûres (entiers, flottants, `bool`,
   `()`), jamais une référence, `String`, `Vec` ou un type nominal. Le code émis
   reste `#![forbid(unsafe_code)]` pour tout programme sans `extern` ; sinon la
   seule construction `unsafe` est le wrapper généré autour de la déclaration
   `extern "C"`, jamais du code recopié de la source Ravum (§3.7 bis).

### 3.7 bis FFI et `unsafe` (sous-ensemble explicite)

| ID | Règle | Niveau |
|---|---|---|
| FFI-1 | `extern "C" { fn f(x: i32) -> i32; }` déclare des fonctions sans corps, au niveau module uniquement. Seule l'ABI `"C"` est acceptée ; une autre ABI est `E-UNSUPPORTED-ABI`. | MUST |
| FFI-2 | Les paramètres et le retour d'une fonction `extern` doivent être FFI-sûrs : `i8..i128`, `u8..u128`, `isize`, `usize`, `f32`, `f64`, `bool`, `()`. Toute autre type est `E-FFI-TYPE`. | MUST |
| FFI-3 | Un appel de fonction `extern` hors d'un bloc `unsafe` est `E-UNSAFE-CONTEXT` ; dans un bloc `unsafe`, il est accepté et produit les effets `unsafe` + `ffi`. | MUST |
| FFI-4 | Un bloc `unsafe` produit l'effet `unsafe` ; le profil `pure` le refuse donc (`E-EFF-PROFILE`). Le profil `rt` le refuse également (profil expérimental inchangé). | MUST |
| FFI-5 | Les pointeurs bruts, `unsafe fn`, `extern "C" fn` avec corps, les attributs `#[no_mangle]`/`#[link]` et les types FFI composés restent `UNSUPPORTED`. | MUST |
| FFI-6 | Le codegen émet `extern "C" { #[link_name = "..."] fn rvm_ffi_<id>(...); }` puis un wrapper `pub fn rvm_ffi_wrap_<id>(...) -> T { unsafe { rvm_ffi_<id>(...) } }` ; les sites d'appel Ravum restent du Rust sûr. Un programme sans `extern` conserve `#![forbid(unsafe_code)]` ; un programme avec `extern` porte `#![deny(unsafe_code)]` et un `#[allow(unsafe_code)]` **local à chaque wrapper** — tout autre `unsafe` fait échouer le build. Le backend rejette aussi toute signature `extern` non primitive (`CodegenError`), même si le checker a été contourné. | MUST |
| FFI-7 | La frontière FFI n'est pas certifiée mémoire : un pointeur invalide fourni par la bibliothèque C reste possible. Toute utilisation FFI doit être auditée et reste hors du profil `pure`. | MUST |

Le modèle est **plus restrictif** que Rust (pas de références stockées/retournées,
pas de lifetimes explicites) : il est sound par construction et documenté comme
sous-ensemble ; les extensions sont listées `UNSUPPORTED` (voir `unsupported.md`).

### 3.8 Runtime, erreurs, panic, sortie

- `fn main()` ou `fn main() -> Result<(), Error>` ; tout autre type de retour est
  `E-TYP-MAIN`. `main -> Result` : `Err(e)` imprime `error: {e}` sur stderr,
  code de sortie 1.
- `Error` : type prélude `struct Error { pub message: String }` avec
  `Error::new(&str)` et implémentation de l'affichage. `io`, `fs` renvoient
  `Result<T, Error>`.
- `?` : sur `Result<T, Error>` dans une fonction renvoyant `Result<U, Error>`,
  sur `Option<T>` dans une fonction renvoyant `Option<U>` (`E-TYP-TRY` sinon).
- Panic : message sur stderr `panic: <message>`, code de sortie **1** (le runtime
  installe un hook, y compris en release). `assert`/`assert_eq` produisent des
  messages avec fichier:ligne:colonne Ravum.
- Arguments : `std::env::args() -> Vec<String>` (effet `io`). E/S : `std::io::print`,
  `std::io::println`, `std::io::eprintln`, `std::io::read_line` ;
  `std::fs::read_to_string`, `std::fs::write_string`, `std::fs::exists` ;
  `std::process::exit(i32)` (effet `io`) ; `std::math` pour `f64` ;
  `std::string`/`std::vec` méthodes de base. Les opérations texte `trim`,
  `to_uppercase`, `to_lowercase`, `replace`, `lines` renvoient des **valeurs
  possédées** (`String`/`Vec<String>`) pour que nulle référence ne soit stockée :
  `trim() -> String`, `lines() -> Vec<String>`. `as_str(&self) -> &str` reste une
  référence strictement temporaire. `assert_eq<T>(a, b)` prend les valeurs
  (références internes) ; il accepte scalaires et texte (mélange `String`/`&str`).
  L'indexation `&[T][i]` et `Vec<T>[i]` est autorisée en lecture (panic hors
  bornes) ; `Vec::as_slice(&self) -> &[T]` permet l'appel `f(v.as_slice())`.
  Les constructeurs de prélude `Some`, `None`, `Ok`, `Err` sont disponibles non
  qualifiés (`Option::Some`, etc. restent acceptés). `std::math` fournit
  `abs sqrt floor ceil round min max powf sin cos tan exp ln log10` sur `f64`
  (précision non garantie pour les fonctions transcendantes, exclues des tests
  bit-exacts). `Error` ne porte qu'un `message: String` ; aucune classification
  programmatique stable en Core 1.0 (limite documentée).

## 4. Architecture d'implémentation (état visé de cette itération)

```
crates/ravum-lang-syntax   source, spans, diagnostics, lexer, AST, parser, fmt
crates/ravum-lang-sema     résolution, types, exhaustivité, ownership, effets,
                           faits typés (CheckedProgram)
crates/ravum-lang-codegen  Rust généré + prélude runtime
crates/ravum-lang-cli      binaire `ravum-lang` (init/check/build/run/test/…)
crates/ravum-lang-lsp      (T7, optionnel ; UNSUPPORTED explicite si absent)
conformance/               corpus déclaratif exécuté par `ravum-lang conformance`
spec/language/             spécification candidate (DRAFT)
```

Interfaces gelées pour le travail parallèle :
- `ast::Program` avec `NodeId` (u32) unique sur **chaque** item, instruction,
  expression, pattern, paramètre, champ, variante, module. Le parser attribue les
  ids dans l'ordre de lecture.
- `sema::facts::CheckedProgram { program: ast::Program, facts: Facts }` où
  `Facts` contient : types de chaque `ExprId`, résolutions de chemins
  (`PathUse`), méthodes résolues, types de patterns, `effects: BTreeMap<FnId,
  EffectSet>`, appels (`CallTarget`), types des `let`/champs, fonctions retenues
  pour les tests. Aucun identifiant non résolu ne subsiste dans un
  `CheckedProgram` valide.
- `codegen::generate(&CheckedProgram, GenOptions) -> Result<String, CodegenError>` :
  produit un unique fichier Rust autonome (prélude inclus), sans dépendance
  externe hors `std`, compilable par `rustc --edition 2021`.

Le code généré conserve un mapping de lignes (commentaires `// rv: <fichier>:<ligne>`
avant chaque item/instruction) pour l'audit ; `--emit-rust` l'écrit.

## 5. Limites explicites de cette itération (UNSUPPORTED, jamais silencieux)

Traits/bornes/HRTB, closures, async/await, threads/atomiques, unsafe/pointeurs
bruts/FFI, macros (déclaratives ou procédurales), réflexion, WASM, GPU, mobile,
hard-RT certifié, hot reload général, LSP complet, registre public de packages,
récursion de types infinie, `Drop` utilisateur, `where`, `dyn`, `impl Trait`,
littéraux de byte, identifiants Unicode, ré-exports, alias d'import, glob,
attributs. Chacun produit un diagnostic `E-UNSUPPORTED-*` explicite.

## 6. Gates de cette itération

- **T0** : baseline reproduite, 4 défauts P0 traités avec tests, aucun chemin
  d'écriture hors workspace dans le corpus adversarial → PASS/PARTIAL honnête.
- **T1** : trois voies comparées sur corpus identique ; au moins une chaîne `.rv`
  réelle parse→check→binaire→run → PASS si harness + résultats liés à un SHA.
- **T2** : spec candidate versionnée + suite de conformance exécutable → PASS si
  chaque règle majeure a un test positif et un négatif exécutables.
- **T3** : lexer/parser robustes (fuzz smoke, entrées hostiles sans panique) et
  diagnostics déterministes → PASS/PARTIAL.
- **T4** : ownership/effets réellement vérifiés sur corpus négatif ; aucun
  programme safe du corpus de réfutation accepté → PASS/PARTIAL avec limites.
- **T5** : binaire natif Windows (machine locale) ; Linux si runner disponible ;
  parité debug/release → PASS si E2E exécuté, sinon PARTIAL/NOT_RUN explicite.
- **T6** : stdlib core/io/fs/String/Vec/Error utilisable depuis `.rv` → PASS/PARTIAL.
- **T7** : init→check→build→run→test→fmt→doc sur machine propre → PASS/PARTIAL.
- **T8** : hot reload `.rv` → NOT_RUN/UNSUPPORTED (restart explicite documenté).
- **T9** : hello, calculator, file-tool, package multi-module, data, DSP offline,
  WASM (selon temps) → statuts par verticale.
- **T10** : conformance/fuzz/audits → PARTIAL avec chiffres réels.
- **T11** : installateur local, checksums, SBOM minimal, docs → PARTIAL/PASS.
- **T12** : dossier Go/No-Go, dogfood, décisions humaines → rapport honnête.

## 7. Discipline d'exécution

Un seul écrivain Git (Prime). Les agents ne modifient que les fichiers de leur
périmètre ; un fichier = un propriétaire à la fois. Aucun test supprimé ou
affaibli. Tout échec est rapporté avec commande, SHA et sortie brute. Aucun
`PROVED`, `production-ready` ou `stable` n'est écrit sans preuve liée.
