# Ravum Core 1.0 — Table exhaustive des fonctionnalités refusées (`UNSUPPORTED`)

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §5 (liste explicite), §3 (surface).**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR**. Aucune implémentation n'est revendiquée (`NOT_IMPLEMENTED`).
> Règle directrice : **aucune fonctionnalité n'est refusée en silence**. Chaque ligne a un code de diagnostic (ou une absence explicite de commande/API), une phase de détection, et une alternative en Core 1.0. Les codes sont ceux de [`diagnostics.md`](diagnostics.md) ; un code `reserved` est marqué `(R)`.

## 1. Pourquoi une table exhaustive

Le design §5 énumère les limites de l'itération. Une liste en prose se perd dans l'implémentation : un backend peut accepter « par accident » une forme non prévue. Cette table est la **liste de fermeture** : elle est référencée par [`features.md`](features.md) et par la conformance négative (T4). Tout test de refus doit citer une ligne de cette table.

| ID | Règle | Niveau |
|---|---|---|
| UNS-0.1 | Toute fonctionnalité absente de la surface Core 1.0 **MUST** figurer ici ou être `E-RES-UNKNOWN` par absence d'API. | MUST |
| UNS-0.2 | Une forme refusée **MUST** produire un diagnostic, jamais un comportement de secours, une inférence silencieuse, ni une exécution partielle. | MUST |
| UNS-0.3 | Une fonctionnalité listée ici **MUST NOT** être acceptée par le backend « parce que `rustc` l'accepte ». Le checker est l'autorité (design §1). | MUST |
| UNS-0.4 | Le message du diagnostic **MUST** nommer la fonctionnalité et renvoyer à ce document (au moins par le code). | SHOULD |

## 2. Items, types et déclarations

| Fonctionnalité | Forme refusée | Code | Phase | Alternative Core 1.0 |
|---|---|---|---|---|
| Traits | `trait T { ... }` | `E-UNSUPPORTED-TRAIT` | parse | `struct`/`enum` + `impl` inhérent |
| Bornes de type | `fn f<T: Ord>(...)` | `E-UNSUPPORTED-TRAIT` | parse | génériques sans bornes |
| `where` | `fn f<T>(...) where T: X` | `E-UNSUPPORTED-WHERE` | parse | aucune |
| `impl Trait` | `fn f() -> impl T` | `E-UNSUPPORTED-IMPL-TRAIT` | parse | type concret ou générique |
| Objets-trait | `Box<dyn T>`, `dyn T` | `E-RES-UNKNOWN` (`dyn` est un identifiant valide, Q-LEX-1) | types | enum fermé |
| HRTB | `for<'a> fn(...)` | `E-UNSUPPORTED-TRAIT` | parse | aucune |
| GAT / types associés | `type Item;` | `E-UNSUPPORTED-TRAIT` | parse | aucune |
| `impl Drop` | `impl Drop for T { fn drop(...) }` | `E-UNSUPPORTED-DROP-IMPL` (R) | resolve | destruction structurelle ; `drop(x)` |
| `static` | `static X: i32 = 1;` | `E-UNSUPPORTED-TOKEN` | parse | `const` (mot-clé réservé, non supporté) |
| Alias de type | `type A = B;` | `E-UNSUPPORTED-TOKEN` | parse | répéter le type (`type` est réservé) |
| `union` | `union U { ... }` | `E-PARSE-EXPECTED` | parse | `struct` + `enum` |
| Const generics | `struct A<const N: usize>` | `E-PARSE-EXPECTED` | parse | tableau `[T; N]` littéral |
| Variadiques | `fn f(x: i32, ...)` | `E-PARSE-EXPECTED` | parse | `Vec<T>` explicite |
| Paramètres de type par défaut | `struct S<T = i32>` | `E-PARSE-EXPECTED` | parse | explicite à chaque usage |
| `#[...]`, `#![...]` | attributs, `derive`, `repr`, `allow` | `E-UNSUPPORTED-ATTR` | parse | aucune (pas de métadonnées) |
| Macros déclaratives | `macro_rules! m { ... }` | `E-UNSUPPORTED-MACRO` | parse | fonction |
| Macros procédurales | `#[proc_macro]` | `E-UNSUPPORTED-MACRO` | parse | aucune |
| Invocations de macro | `println!(...)`, `vec![...]` | `E-PARSE-UNEXPECTED` (macro) | parse | fonctions `std` |
| `impl` sur type étranger | `impl i32 { ... }` | `E-RES-IMPL-SELF-TYPE` | resolve | fonction libre |
| Types récursifs infinis | `struct N { next: Option<N> }` | `E-TYP-RECURSIVE` (R) | types | `Vec<T>` (indirection interne) |
| `str` seul | `fn f(s: str)` | `E-RES-UNKNOWN` | resolve | `&str` |
| `Box<T>`, `Rc<T>`, `Cell<T>` | toute utilisation | `E-RES-UNKNOWN` | resolve | `String`/`Vec`/types nominaux |
| Type `!` nommé en retour non divergent | `fn f() -> !` avec corps qui termine | `E-TYP-MISMATCH` | types | `!` réservé à `exit`/`panic` |

```rv
// NÉGATIF (cumul) — E-UNSUPPORTED-TRAIT, E-UNSUPPORTED-WHERE, E-UNSUPPORTED-ATTR
trait Affichable { fn affiche(&self) -> String; }
fn f<T>(x: T) -> T where T: Affichable { return x; }
#[derive(Debug)]
struct P { x: i32 }
```

## 3. Instructions et expressions

| Fonctionnalité | Forme refusée | Code | Phase | Alternative Core 1.0 |
|---|---|---|---|---|
| Closures | `|x| x + 1`, `move |x| ...` | `E-UNSUPPORTED-CLOSURE` | parse | fonction nommée ; pas de capture |
| `async`/`await` | `async fn`, `.await` | `E-UNSUPPORTED-ASYNC` | parse | aucune |
| `unsafe` | `unsafe { }`, `unsafe fn` | `E-UNSUPPORTED-UNSAFE` | parse | aucune |
| Pointeurs bruts | `*const T`, `*mut T`, `&raw` | `E-UNSUPPORTED-UNSAFE` | parse | références `&T`/`&mut T` |
| Décalage de pointeur | `ptr.offset(n)` | `E-RES-UNKNOWN` | resolve | indexation |
| `transmute` | `transmute(a)`, `as` interdit entre agrégats | `E-TYP-CAST` | types | aucune |
| `extern` / FFI | `extern "C"`, `#[no_mangle]`, `libc` | `E-UNSUPPORTED-EXTERN` | parse | aucune |
| Ranges hors `for` | `let r = 0..n;`, `..=`, `&v[1..2]`, `..base` | `E-UNSUPPORTED-RANGE` | parse | `for i in a..b` uniquement |

| `break` avec valeur | `break e;` | `E-PARSE-EXPECTED` | parse | `loop` + variable mutable |
| Étiquettes de boucle | `'outer: loop {}`, `break 'outer;` | `E-UNSUPPORTED-TOKEN` | parse | sortie par condition / `return` |
| Turbofish | `f::<i32>(x)`, `Vec::<i32>::new()` | `E-PARSE-UNEXPECTED` | parse | annotation `let v: Vec<i32>` |
| Shorthand de champ en expression | `P { x }` (expression) | `E-PARSE-EXPECTED` | parse | `P { x: x }` |
| Mise à jour de struct | `P { x: 1, ..old }` | `E-UNSUPPORTED-RANGE` | parse | affecter les champs |
| Pattern de `let` | `let (a, b) = t;` | `E-PARSE-EXPECTED` | parse | `match` |
| Pattern de paramètre | `fn f((a, b): (i32, i32))` | `E-PARSE-EXPECTED` | parse | paramètres séparés / `match` |
| UFCS | `Type::methode(recv, ...)` | `E-RES-UNKNOWN` | resolve | `recv.methode(...)` |
| Appel de module avec point | `io.println("x")` | `E-RES-UNKNOWN` | resolve | `io::println("x")` |
| `+` sur `String` | `a + b` (texte) | `E-TYP-EQ`/`E-TYP-MISMATCH` | types | `push_str` |
| Surcharge d'opérateurs | `impl Add for T` | `E-UNSUPPORTED-TRAIT` | parse | méthodes nommées |
| Cast de conversion texte | `s as i64`, `x as String` | `E-TYP-CAST` | types | `std::string::parse_*`, `to_string` |
| Indexation de `String` | `s[0]` | `E-TYP-INDEX` | types | `lines`, `contains`, `as_str` |

```rv
// NÉGATIF (cumul) — E-UNSUPPORTED-RANGE, E-PARSE-EXPECTED (compound), E-RES-UNKNOWN (point)
fn f(v: Vec<i32>) {
    let r = 0..3;
    let mut n = 1;
    n *= 2;
    io.println("x");
}
```

## 4. Patterns et exhaustivité

| Fonctionnalité | Forme refusée | Code | Phase | Alternative Core 1.0 |
|---|---|---|---|---|
| Pattern de référence | `ref x`, `&x` en pattern | `E-UNSUPPORTED-TOKEN` | parse | capture par valeur (`match` consomme) |
| Pattern ou | `1 | 2 => ...` | `E-UNSUPPORTED-TOKEN` | parse | deux bras |
| Garde de pattern | `n if n > 0 => ...` | `E-UNSUPPORTED-TOKEN` | parse | `if` dans le corps |
| `@` (binding) | `x @ 1..=5` | `E-UNSUPPORTED-TOKEN` | parse | bras séparés |
| Pattern de plage | `1..=5 => ...` | `E-UNSUPPORTED-RANGE` | parse | `_` + `if` |
| Pattern de struct | `Point { x, y }` | `E-PARSE-EXPECTED` | parse | accès de champ / méthode |
| Reste de tuple | `(a, ..)` | `E-UNSUPPORTED-RANGE` | parse | tuple complet |
| `match` sur `String` | `match s { "a" => ... }` avec `s: String` | `E-TYP-EQ` | types | `match s.as_str()` |

> Les patterns de **littéraux flottants** sont **acceptés** (sémantique `==`, `NaN` ne correspond jamais ; `-0.0` correspond à `0.0`). Voir [`grammar.md`](grammar.md) G-6.36.

## 5. Effets, concurrence, temps réel

| Fonctionnalité | Forme refusée | Code | Phase | Alternative Core 1.0 |
|---|---|---|---|---|
| Effet `net` | `effects(net)`, toute API réseau | `E-UNSUPPORTED-EFFECT` | effects | aucune |
| Effet `unsafe` | `effects(unsafe)` | `E-UNSUPPORTED-EFFECT` | effects | aucune |
| Effet `ffi` | `effects(ffi)` | `E-UNSUPPORTED-EFFECT` | effects | aucune |
| Effet `async` | `effects(async)`, `async fn` | `E-UNSUPPORTED-EFFECT` / `E-UNSUPPORTED-ASYNC` | effects/parse | aucune |
| Threads | `thread::spawn`, `JoinHandle` | `E-RES-UNKNOWN` | resolve | mono-thread |
| Atomiques | `AtomicUsize`, `Ordering::*` | `E-RES-UNKNOWN` | resolve | aucune |
| Synchronisation | `Mutex`, `RwLock`, `Condvar`, `Arc` | `E-RES-UNKNOWN` | resolve | aucune |
| Canaux | `std::sync::mpsc` | `E-RES-UNKNOWN` | resolve | fichiers/E-S |
| Annulation / timeout | `select`, `recv_timeout` | `E-RES-UNKNOWN` | resolve | aucune |
| Hard-RT certifié | profil `hard_rt`, WCET | `E-PARSE-EXPECTED` (profil inconnu → exit `2`) | CLI | `rt` **expérimental**, non certifié |
| Thread audio / callback | API hôte, `process` audio | `E-RES-UNKNOWN` | resolve | hors périmètre |
| Hot reload / hot swap | chargement de module compilé | `UNSUPPORTED` explicite (T8) | CLI | redémarrage manuel |
| Profil `total` / terminaison garantie | `@total`, preuve de variant | `E-UNSUPPORTED-ATTR` | parse | aucune garantie |

> Le groupe d'effet `rt` est **déclarable** mais sans sémantique ([`effects.md`](effects.md) EFF-1.1, Q-EFF-3) : il ne certifie rien.

## 6. Modules, packages, écosystème

| Fonctionnalité | Forme refusée | Code | Phase | Alternative Core 1.0 |
|---|---|---|---|---|
| Import glob | `use a::*;` | `E-UNSUPPORTED-TOKEN` | parse | imports nommés |
| Alias d'import | `use a as b;` | `E-UNSUPPORTED-TOKEN` | parse | nom d'origine |
| Ré-export | `pub use ...;` | `E-UNSUPPORTED-TOKEN` | parse | `pub` sur l'item d'origine |
| Import de variante | `use Option::Some;` | `E-RES-UNKNOWN` | resolve | qualifier à l'usage |
| Import groupé | `use std::io::{print, println};` | `E-PARSE-UNEXPECTED` | parse | un `use` par nom/module |
| `extern crate` | `extern crate foo;` | `E-UNSUPPORTED-EXTERN` | parse | `use foo::...` |
| Bibliothèque du même package | binaire important `src/lib.rv` | `E-RES-UNKNOWN` | resolve | deux packages, ou dépendance locale |
| Dépendance distante | `git`, `registry`, `version` seule | `E-MOD-DEPENDENCY` (R) | modules | `path` local |
| Registre public de packages | publication, téléchargement | `UNSUPPORTED` (aucune commande) | CLI | chemins locaux |
| Plages de versions | `^1.2`, `~1`, `>=1` | `E-MOD-DEPENDENCY` (R) | modules | version exacte, chemin |
| Multi-version d'un package | deux versions du même nom | `E-MOD-DEPENDENCY` (R) | modules | une seule version |
| Build script | `build.rs`, `[build]` | `E-MOD-BUILD-SCRIPT` (R) | modules | aucune exécution à l'import |
| Plugins de compilateur | extension du compilateur | `E-UNSUPPORTED-MACRO` | parse | aucune |
| Liens symboliques | dépendance/module via symlink | `E-MOD-PATH` | modules | chemin réel |
| Noms réservés Windows | `CON`, `NUL`, `COM1`… dans un chemin | `E-MOD-RESERVED-NAME` | modules | nom de chemin portable |
| Chemins absolus | `path = "C:\\x"` | `E-MOD-PATH-ESCAPE` | modules | chemin relatif au manifeste |

## 7. Plateformes et outillage

| Fonctionnalité | Forme refusée / état | Manifestation | Alternative Core 1.0 |
|---|---|---|---|
| WASM | cible `wasm32`, export WASM | `UNSUPPORTED` (T9, selon temps) | natif uniquement |
| GPU / accélérateurs | kernels, `gpu` effect | `UNSUPPORTED` (design §5) | CPU |
| Mobile | cibles iOS/Android | `UNSUPPORTED` | desktop |
| Embarqué / `no_std` | runtime absent | `UNSUPPORTED` (design §5) | runtime `std` |
| Hard-RT certifié | WCET, plateforme qualifiée | `UNSUPPORTED` | `rt` expérimental |
| LSP complet | protocole LSP | `UNSUPPORTED` explicite si absent (`lsp`) | `check --json` |
| `--emit-ir` | IR propre | `UNSUPPORTED` explicite (voie C non qualifiée) | `--emit-rust` |
| Débogueur intégré | DAP, breakpoints | `UNSUPPORTED` | journaux |
| Formateur configurable | options `fmt` | partiel (LF + indentation) | `fmt` par défaut |
| Couverture / profilage | instrumentation | `UNSUPPORTED` | mesures externes |
| Télémétrie réseau | envoi de données | `UNSUPPORTED` (privacy by default) | aucune |
| Publication / signature | releases, checksums | hors périmètre spec | — |
| Binaire auto-hébergé | compilateur en Ravum | non prévu | `rustc` + binaire Rust |

## 8. Ce qui n'est **pas** refusé (précisions)

Ces éléments sont souvent supposés `UNSUPPORTED` à tort :

| Élément | Statut réel |
|---|---|
| `!` (type Never) | **supporté**, coercible vers tout type (TYP-1.6) |
| Génériques sans bornes | supporté (monomorphisation) |
| `Option`/`Result` + `?` | supporté |
| Textes Unicode dans les **littéraux** | supporté (UTF-8) |
| `for` sur `a..b` **et** sur `Vec<T>` | supporté (G-6.24) |
| `&[T]` via coercition `&[T; N]` | supporté |
| Patterns de littéraux flottants | supporté |
| `test fn` avec `-> Result<(), Error>` | supporté (G-3.6) |
| `drop(x)` | supporté (intrinsèque) |
| `pub` sur une variante d'enum | accepté sans effet (G-4.2) |
| `//!` et `///` | acceptés (trivia, `doc`) |

## 9. Procédure pour proposer une extension

| ID | Règle | Niveau |
|---|---|---|
| UNS-9.1 | Une extension **MUST** être proposée comme RFC/ADR (jamais acceptée par un agent). | MUST |
| UNS-9.2 | La proposition **MUST** préciser : syntaxe, règle de typage, règle d'ownership, effets, impact sur `features.md`, tests positifs **et** négatifs, statut de compatibilité. | MUST |
| UNS-9.3 | Une fonctionnalité `UNSUPPORTED` qui devient supportée **MUST** être retirée de cette table **et** ajoutée à [`features.md`](features.md) avec un `FeatureId` et un cas de conformance. | MUST |
| UNS-9.4 | Une fonctionnalité **MUST NOT** être activée « temporairement » ou « en interne » : le checker est unique. | MUST |
| UNS-9.5 | La compatibilité de l'extension est régie par [`compatibility.md`](compatibility.md) §2. | MUST |

## 10. Questions ouvertes

- **Q-UNS-1 (résolue par le Lead)** — Tous les codes nécessaires sont désormais canoniques (`E-MOD-MANIFEST/DEPENDENCY/BUILD-SCRIPT/LOCK/BOTH-ROOTS/PATH-ESCAPE/RESERVED-NAME`, `E-OWN-IMMUTABLE`, `E-TYP-RECURSIVE/CONST`, `E-UNSUPPORTED-DROP-IMPL`) ; `E-RES-VISIBILITY` est remplacé par `E-RES-PRIVATE`. Aucun code réservé.
- **Q-UNS-2 (résolue par le Lead)** — `static` et `type` sont **réservés** (`E-UNSUPPORTED-TOKEN` à l'usage) ; `ref`, `dyn`, `await` sont des **identifiants valides** : `dyn T` → `E-RES-UNKNOWN`, `x.await` → `E-RES-UNKNOWN`, `ref x` en pattern → `E-PARSE-EXPECTED`.
- **Q-UNS-3** — Les fonctionnalités de plateforme (WASM, GPU, mobile, `no_std`) n'ont **aucun** code de diagnostic : elles sont absentes du CLI. Faut-il une sous-commande `ravum-lang targets` listant explicitement `UNSUPPORTED` pour éviter l'ambiguïté « ça n'existe pas » vs « ce n'est pas fait » ?
- **Q-UNS-4** — `impl Trait`, `dyn`, `where` sont refusés avec des codes distincts ; `Box`/`Rc` avec `E-RES-UNKNOWN`. Cette granularité hétérogène est-elle acceptable pour les agents qui réparent ? Recommandation : oui, mais le message doit toujours nommer la fonctionnalité.
- **Q-UNS-5** — Le hot swap est `UNSUPPORTED` alors qu'il est un objectif produit (`docs/specs/03`). Il doit rester hors de `spec/language/` (c'est une propriété de runtime/daemon) ; à clarifier dans le design pour éviter une promesse implicite.
