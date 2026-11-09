# Ravum Core 1.0 — Effets (capacités) et profils

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.6, §3.3, §5.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR** (notamment pas ADR-005 « système d'effets et capacités » ni ADR-006 « temps réel »). Aucune implémentation n'est revendiquée (`NOT_IMPLEMENTED`).

## 1. Groupes d'effets

Neuf groupes, exactement ceux du design §3.6 :

| Groupe | Sens | Inféré en Core 1.0 ? | Déclarable ? |
|---|---|---|---|
| `io` | entrée/sortie standard (`print`, `println`, `eprintln`, `read_line`, `process::exit`) | oui | oui |
| `fs` | système de fichiers (`read_to_string`, `write_string`, `exists`) | oui | oui |
| `alloc` | allocation dynamique (`String`, `Vec`, `Error::new`, `clone`, conversions texte) | oui (granularité fonction) | oui |
| `panic` | arrêt contrôlé (`panic`, `assert`, `assert_eq`, `unwrap`) | oui | oui |
| `nondet` | non-déterminisme (réservé) | **non** (aucune source en Core 1.0) | oui, sans effet |
| `rt` | appartenance à un contexte temps réel (réservé) | **non** | oui, sans effet |
| `net` | réseau | **non implémenté** | refusé : `E-UNSUPPORTED-EFFECT` |
| `unsafe` | code non vérifié | **non implémenté** | refusé : `E-UNSUPPORTED-EFFECT` |
| `ffi` | frontière étrangère | **non implémenté** | refusé : `E-UNSUPPORTED-EFFECT` |
| `async` | exécution asynchrone | **non implémenté** | refusé : `E-UNSUPPORTED-EFFECT` |

> Le design §3.6 énumère `io fs net alloc panic unsafe ffi async rt nondet` (9 groupes) et précise que « `unsafe`/`ffi`/`net`/`async` ne sont pas implémentés en Core 1.0 : les déclarer est `E-UNSUPPORTED-EFFECT` ». `nondet` et `rt` ne sont pas dans cette liste d'exclusion ; ils sont donc **déclarables mais jamais inférés** (voir Q-EFF-3).

| ID | Règle | Niveau |
|---|---|---|
| EFF-1.1 | Un ensemble d'effets est un sous-ensemble des 9 groupes ; il n'existe aucun effet personnalisé, aucune capacité paramétrée, aucun niveau de granularité. | MUST |
| EFF-1.2 | Un nom d'effet inconnu (`effects(network)`, `effects(io, memory)`) est `E-EFF-UNKNOWN` (span du nom). | MUST |
| EFF-1.3 | Un effet répété dans la même liste est accepté **sans diagnostic** : l'ensemble est idempotent. | MUST |
| EFF-1.4 | `effects()` (liste vide) déclare l'ensemble vide : la fonction ne peut atteindre **aucun** effet, y compris `alloc` et `panic`. | MUST |
| EFF-1.5 | L'absence totale de clause `effects(...)` signifie « **ensemble inféré public** » : la fonction est utilisable partout, et son ensemble d'effets est celui calculé par le checker, exposé dans les faits (`CheckedProgram.facts.effects`). | MUST |
| EFF-1.6 | L'ordre des groupes dans `effects(...)` est libre et sans signification ; l'ensemble est canoniquement trié pour l'affichage et le JSON. | MUST |

```rv
// POSITIF
fn pure_add(a: i32, b: i32) -> i32 effects() { return a + b; }
fn affiche(s: &str) effects(io) { io::println(s); }
fn charge(p: &str) -> Result<String, Error> effects(fs, io, alloc) { return fs::read_to_string(p); }
fn reservé() effects(fs, rt) { }          // `rt` déclarable, jamais inféré
```

```rv
// NÉGATIF — E-EFF-UNKNOWN
fn f() effects(network) { }
// NÉGATIF — E-UNSUPPORTED-EFFECT (groupe non implémenté)
fn g() effects(net) { }
fn h() effects(unsafe) { }
fn i() effects(ffi) { }
fn j() effects(async) { }
```

## 2. Inférence transitive

| ID | Règle | Niveau |
|---|---|---|
| EFF-2.1 | L'ensemble d'effets inféré d'une fonction est l'union : (a) des effets des fonctions **appelées** (transitivement), (b) des effets des méthodes appelées, (c) des effets des intrinsèques et fonctions du prélude, (d) de `alloc` si le corps **construit** un `String`, un `Vec`, un `Box` (inexistant), ou appelle une fonction allouante. | MUST |
| EFF-2.2 | L'inférence est un **point fixe minimal** sur le graphe d'appels statique. La récursion (directe ou mutuelle) est gérée : l'ensemble d'une fonction récursive est le plus petit point fixe incluant ses appels. | MUST |
| EFF-2.3 | Les **arguments** ne « transmettent » pas d'effets : passer une `&str` à une fonction n'ajoute pas d'effet à l'appelant. Seuls les effets **de l'appel** comptent. | MUST |
| EFF-2.4 | Le graphe d'appels est **statique et complet** : il n'existe ni pointeur de fonction, ni closure, ni `dyn`, ni dispatch dynamique. L'inférence est donc exacte sur le programme, sans approximation par sur-ensemble. | MUST |
| EFF-2.5 | La granularité de `alloc` est **la fonction** (design §3.6) : toute fonction qui peut allouer est marquée `alloc`, même si l'allocation ne se produit que sur un chemin d'erreur (ex. `parse_i64`). Aucune analyse de chemin n'est faite pour `alloc`. | MUST |
| EFF-2.6 | Une fonction **générique** infère l'union des effets sur toutes ses instanciations monomorphisées ; une instanciation qui appelle une fonction à effet `io` contamine l'instanciation, pas la définition générique abstraite. Le fait exposé est l'union par `FnId`. | MUST |
| EFF-2.7 | `drop(x)` n'ajoute **aucun** effet. Sont **purs** : `math::*` (y compris `sin`, `cos`, `tan`, `exp`, `ln`, `log10`), `len`, `is_empty`, `as_slice`, `contains`, `starts_with`, `ends_with`, `is_some`, `is_none`, `is_ok`, `is_err`, `pop`, `unwrap_or`, `ok`, `err`, `ok_or`. Ajoutent `alloc` : `String::new/from/clone/push/push_str`, `Vec::new/with_capacity/push/clone`, `to_string` (méthodes primitives), `string::from`, `parse_*`, `trim`/`to_uppercase`/`to_lowercase`/`replace`/`lines`, `Error::new`, `fs::read_to_string`, `env::args`/`env::var`, `io::read_line`. Ajoutent `panic` : `panic`, `assert`, `assert_eq`, `unwrap`, `unwrap_err`. `io` : `print`, `println`, `eprintln`, `read_line`, `exit`, `args`, `var`. `fs` : les trois fonctions `fs`. | MUST |
| EFF-2.8 | L'inférence est **indépendante de l'ordre des déclarations** et **déterministe** : deux exécutions produisent le même `EffectSet`. | MUST |

```rv
// POSITIF — transitivité
fn a() effects(io) { io::println("a"); }
fn b() effects(io) { a(); }                        // io inféré
fn c() effects(io, alloc) { b(); let s = String::from("x"); io::println(s.as_str()); }

// POSITIF — récursion
fn rec(n: i32) effects(io) { if n > 0 { rec(n - 1); } else { io::println("fin"); } }
```

```rv
// NÉGATIF — E-EFF-UNDECLARED (appel à effet io non déclaré)
fn f() effects() { io::println("x"); }
// NÉGATIF — E-EFF-UNDECLARED (transitif)
fn g() effects() { f(); }
// NÉGATIF — E-EFF-UNDECLARED (alloc non déclaré)
fn h() effects() -> String { return String::from("x"); }
```

## 3. Déclaration majorante

| ID | Règle | Niveau |
|---|---|---|
| EFF-3.1 | Une déclaration `effects(S)` est un **majorant** : elle doit contenir l'ensemble inféré du corps (`inféré ⊆ déclaré`). Sinon `E-EFF-UNDECLARED`, avec la liste des effets manquants. | MUST |
| EFF-3.2 | Une déclaration **plus large** que nécessaire est légale (contrat volontairement conservateur) et ne produit **aucun diagnostic** : il n'existe pas d'avertissement `W-EFF-*`. | MUST |
| EFF-3.3 | La déclaration ne **réduit** jamais les effets : il n'existe aucun `effects(none)` qui « efface » un effet inféré. Aucun cast de capacité, aucun `assume`. | MUST |
| EFF-3.4 | Un appel à une fonction dont la déclaration ne couvre pas ses propres effets est refusé à la **définition** de cette fonction (`E-EFF-UNDECLARED`), pas à l'appel. | MUST |
| EFF-3.5 | Le diagnostic `E-EFF-UNDECLARED` pointe l'appel fautif **et** la clause `effects` de la fonction, avec une note listant les effets manquants dans l'ordre canonique. | MUST |
| EFF-3.6 | `E-EFF-UNDECLARED` est une erreur, jamais un avertissement ; un profil ne peut pas l'abaisser en warning. | MUST |

```rv
// POSITIF — majorant plus large
fn f() effects(io, alloc) { io::println("x"); }    // alloc déclaré mais non inféré : légal, silencieux
// POSITIF — majorant exact
fn g() effects(io) { io::println("x"); }
```

```rv
// NÉGATIF — E-EFF-UNDECLARED
fn h() effects(io) { let s = String::from("x"); io::println(s.as_str()); }  // alloc manquant
// NÉGATIF — E-EFF-UNDECLARED (déclaration vide)
fn i() effects() { let v: Vec<i32> = Vec::new(); }
```

## 4. Profils

Un profil est un **mode de vérification** choisi par `--profile` (`check`, `build`, `run`, `test`), pas une annotation source.

| Profil | Statut | Ce qu'il vérifie | Effets interdits |
|---|---|---|---|
| `core` (défaut) | candidate-stable | aucun contrôle supplémentaire ; le programme doit être accepté par le checker standard | — |
| `pure` | candidate-stable | le **point d'entrée** (`main`, ou la fonction ciblée par `test`) ne peut atteindre **aucun** effet interdit, transitivement | `io fs net alloc panic unsafe ffi async rt nondet` |
| `rt` | **expérimental** | le corps et la fermeture transitive des fonctions atteignables depuis l'entrée ne contiennent aucun effet interdit ; ne certifie **pas** le hard-RT | `io fs net alloc panic unsafe ffi async rt` |

| ID | Règle | Niveau |
|---|---|---|
| EFF-4.1 | `core` n'interdit aucun effet. C'est le profil par défaut. | MUST |
| EFF-4.2 | `pure` exige que le point d'entrée n'atteigne **aucun** des neuf effets interdits (design §3.6, formulation corrigée — voir Q-EFF-1). Un seul effet atteint est `E-EFF-PROFILE`. | MUST |
| EFF-4.3 | Sous `pure`, une fonction **non atteignable** depuis l'entrée peut avoir des effets : elle n'est pas vérifiée. Le profil est **contextuel à l'entrée**, pas global. | MUST |
| EFF-4.4 | `pure` interdit `alloc` : un programme `pure` ne peut donc utiliser ni `String`, ni `Vec`, ni aucune conversion texte, ni `println`. Il est limité aux scalaires, tuples, tableaux, structs/enums `Copy` et mathématiques `f64`. | MUST |
| EFF-4.5 | `pure` n'implique **pas** l'absence de panic runtime : le débordement, la division par zéro et l'index hors bornes peuvent toujours survenir (les traps arithmétiques ne sont pas des effets). Un programme `pure` peut terminer par un panic (voir Q-EFF-2). | MUST |
| EFF-4.6 | `rt` est **expérimental** : il est vérifié, mais aucun résultat ne peut être présenté comme certification temps réel, WCET, ni hard-RT (design §3.6 ; `docs/specs/04-temps-reel.md`). Le libellé d'un rapport doit contenir `PARTIAL`/`EXPERIMENTAL`, jamais `RT-CERTIFIED`. | MUST |
| EFF-4.7 | `rt` interdit `io fs net alloc panic unsafe ffi async rt`. Comme `unsafe`/`ffi`/`net`/`async` sont déjà inacceptables à la déclaration, les interdits effectifs sont `io fs alloc panic` (+ `net`/`rt` si déclarés). | MUST |
| EFF-4.8 | Aucun profil n'est une **garantie de sécurité d'exécution** : voir §6. | MUST |
| EFF-4.9 | Le profil ne change **jamais** la sémantique du programme accepté : il ne fait qu'ajouter des refus. Deux programmes identiques acceptés sous `core` et `pure` ont le même comportement. | MUST |
| EFF-4.10 | Un profil inconnu (`--profile hard_rt`) est un usage invalide : code de sortie `2` (et non `1`). | MUST |

```rv
// POSITIF — profil `pure` : aucune API à effet ; `assert` est interdit (`panic`)
fn main() {
    let mut s: i64 = 0;
    for i in 0..10 { s += i; }
    let mut t: i64 = 0;
    for i in 0..s { t += i; }
}

// POSITIF — profil rt (expérimental)
fn process(x: f64, y: f64) -> f64 effects() { return x * 0.5 + y * 0.5; }

// NÉGATIF — E-EFF-PROFILE (io sous pure)
fn main() { io::println("x"); }
// NÉGATIF — E-EFF-PROFILE (alloc sous pure)
fn main() { let s = String::from("x"); }
// NÉGATIF — E-EFF-PROFILE (panic sous pure, via assert)
fn main() { assert(1 == 2); }
// NÉGATIF — E-EFF-PROFILE (fs sous rt, transitif)
fn helper() effects(fs) { let b = fs::exists("x"); }
fn process2() effects() { helper(); }
```

## 5. Annotation vs enforcement

| ID | Règle | Niveau |
|---|---|---|
| EFF-5.1 | La clause `effects(...)` est une **annotation contractuelle vérifiée statiquement**. Elle n'a **aucun effet d'exécution** : aucun jeton de capacité n'est passé, aucune vérification runtime n'a lieu. | MUST |
| EFF-5.2 | Le checker est **l'autorité** : il refuse un programme dont une déclaration est trop faible. Le backend (`rustc`) ne revérifie pas les effets et ne peut pas « rattraper » une déclaration fausse. | MUST |
| EFF-5.3 | Une déclaration fausse n'est **jamais** corrigée silencieusement (ni élargie automatiquement, ni ignorée). | MUST |
| EFF-5.4 | Les effets des fonctions du prélude sont **fixés par la spécification** ([`stdlib.md`](stdlib.md)) et ne peuvent pas être redéclarés par l'utilisateur. | MUST |
| EFF-5.5 | L'ensemble inféré est exposé dans les faits (`CheckedProgram.facts.effects: BTreeMap<FnId, EffectSet>`) et dans `check --json` (`facts.effects`), pour que l'IDE et les agents puissent l'afficher sans relancer l'analyse. | MUST |
| EFF-5.6 | Aucun mécanisme d'`unsafe`-effets, de dérogation, ni de `#[allow(effects)]` n'existe (les attributs sont refusés). | MUST |

```rv
// POSITIF — la déclaration est un contrat vérifié, pas un jeton runtime
fn log(s: &str) effects(io) { io::println(s); }
fn main() { log("x"); }                      // io inféré sur main, autorisé sous core
```

```rv
// NÉGATIF — E-EFF-UNDECLARED (déclaration trop faible, non corrigée)
fn log(s: &str) effects() { io::println(s); }
```

## 6. Limites explicites

| ID | Règle | Niveau |
|---|---|---|
| EFF-6.1 | Il n'existe **aucun bac à sable natif** : un programme `core` peut lire/écrire n'importe quel fichier accessible au processus (`fs`), écrire sur stdout/stderr (`io`), et consommer toute la mémoire disponible (`alloc`). Le profil `pure` est un refus **statique**, pas une isolation. | MUST |
| EFF-6.2 | `fs` ne restreint **aucun chemin** : `../`, chemins absolus, liens symboliques sont acceptés à l'exécution (les restrictions de chemins ne concernent que la résolution des **dépendances**, [`modules.md`](modules.md) MOD-5.6). | MUST |
| EFF-6.3 | Les effets ne garantissent **ni** la terminaison, **ni** une borne de temps, **ni** une borne mémoire, **ni** l'absence de fuite. Un programme sans effets peut boucler indéfiniment. | MUST |
| EFF-6.4 | **Approximation assumée** : les panics **intrinsèques** (débordement, division/reste par zéro, décalage hors largeur, index hors bornes) ne sont **pas** suivis comme effet `panic` ; seules les API explicites (`panic`, `assert`, `assert_eq`, `unwrap`, `unwrap_err`) le sont. Une fonction `effects()` peut donc terminer par un panic contrôlé, et le profil `pure` ne garantit pas l'absence de panic (Q-EFF-2). | MUST |
| EFF-6.5 | `alloc` à granularité fonction est une **sur-approximation** : une fonction marquée `alloc` peut ne jamais allouer sur un chemin donné. Inversement, aucune allocation cachée n'est ignorée : toute construction `String`/`Vec` est marquée. | MUST |
| EFF-6.6 | Aucune garantie de non-fuite mémoire n'est donnée : sans `Drop` utilisateur, la libération est structurelle, mais l'usage de `process::exit` termine sans destructeurs. | MUST |
| EFF-6.7 | Un rapport de profil doit déclarer la **version de spécification** (`core-1.0-draft.1`), le profil, le point d'entrée, et le résultat (`PASS`/`PARTIAL`/`FAIL`), jamais `PROVED`. | MUST |

```rv
// POSITIF — effets vides mais panic arithmétique possible (EFF-6.4)
fn div(a: i32, b: i32) -> i32 effects() { return a / b; }

// NÉGATIF — E-EFF-PROFILE (io sous pure, même indirect)
fn helper() effects(io) { io::println("x"); }
fn main() { helper(); }
```

## 7. Interaction avec les autres modules de la spécification

| Aspect | Référence |
|---|---|
| Effets des fonctions du prélude | [`stdlib.md`](stdlib.md) — colonne « effets » de chaque signature |
| Effets et codegen | [`abi.md`](abi.md) §4 — le backend ne réintroduit aucun effet non déclaré |
| Effets et modules | [`modules.md`](modules.md) MOD-6.4 — pas d'effet au niveau module |
| Effets et diagnostics | [`diagnostics.md`](diagnostics.md) §3 — codes `E-EFF-*` |
| Effets et machine abstraite | [`core-machine.md`](core-machine.md) §9 — le noyau pur est exactement le sous-ensemble sans effet |

## 8. Questions ouvertes

- **Q-EFF-1 (résolue par le Lead)** — Le design §3.6 a été corrigé : `pure` exige que l'entrée n'atteigne **aucun** effet interdit (EFF-4.2).
- **Q-EFF-2 (résolue par le Lead)** — Approximation **assumée et documentée** (EFF-6.4) : les traps intrinsèques ne sont pas des effets `panic`, donc un programme `pure` peut terminer par un panic contrôlé. Aucune garantie d'absence de panic n'est offerte par `pure`.
- **Q-EFF-3** — `nondet` et `rt` sont déclarables mais jamais inférés : leur sémantique est vide en Core 1.0. Faut-il les refuser aussi (`E-UNSUPPORTED-EFFECT`) pour éviter une fausse impression de garantie ? Recommandation : les garder déclarables (compatibilité ascendante) mais documenter qu'ils ne prouvent rien.
- **Q-EFF-4** — `process::exit(i32)` : classé `io` par défaut. Il devrait peut-être avoir son propre effet (`exit`) ou appartenir à `nondet`. La liste des groupes du design ne le prévoit pas.
- **Q-EFF-5 (résolue par le Lead)** — `env` et `process` n'ont **pas** de groupe propre : `env::args`, `env::var`, `process::exit` sont classés `io` (+ `alloc` pour `args`/`var`).
- **Q-EFF-6** — Le profil `rt` vérifie « le corps et la fermeture transitive » : ce document applique la vérification aux fonctions **atteignables depuis l'entrée**. Faut-il vérifier **toutes** les fonctions du package (plus strict, plus prévisible) ? Recommandation : toutes les fonctions du package pour `rt`, car un DSP peut être appelé par un hôte non-Ravum (impossible en Core 1.0, mais l'intention est plus sûre).
- **Q-EFF-7** — `alloc` à granularité fonction rend `pure` inutilisable pour tout ce qui manipule du texte. C'est conforme au design mais rend `pure` très étroit. Confirmer que c'est l'intention (utile pour l'oracle de [`core-machine.md`](core-machine.md)).
- **Q-EFF-8 (résolue par le Lead)** — Une déclaration plus large est légale et silencieuse (EFF-3.2) ; aucun `W-EFF-*` n'existe. Le checker expose l'ensemble **inféré** dans les faits (`facts.effects`) et dans `check --json`, ce qui permet à l'outillage de signaler l'écart sans diagnostic.
