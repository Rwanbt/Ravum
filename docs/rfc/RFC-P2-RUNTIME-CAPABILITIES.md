# RFC-P2-A — Modèle de runtime modulaire : capacités, effets, profils

> **Statut : PROPOSED — non ratifié.** Ce document **ne ratifie aucun ADR** et
> **ne déclare aucune fonctionnalité nouvelle comme supportée**. Il décrit ce qui
> existe, mesure ce qui est mesurable, et nomme ce qui manque.
>
> | Élément | Valeur |
> |---|---|
> | Date | 2026-11-11 |
> | Branche / HEAD | `feat/lang-core-1.0-rc` @ `5d88ecb` |
> | Version de spécification de référence | `core-1.0-draft.1` (DRAFT, non ratifiée) |
> | Plateforme des mesures | Windows 11 x86-64, i7-13620H, 16 threads logiques, ≈15,7 Go RAM |
> | Toolchain | `rustc 1.95.0`, `cargo 1.95.0`, MSVC `14.50.35717` |
> | Portée | P2-A de la décomposition P2 de la session 2026-11-11 |
>
> **Vocabulaire de statut utilisé ici, sans exception** : `IMPLEMENTED` (du code
> exécutable existe et a été exécuté), `DECLARED_ONLY` (le nom existe, aucune
> sémantique), `REFUSED` (rejet explicite avec un code), `ABSENT` (aucun nom,
> aucune API). Aucun autre statut n'est employé, et aucun `IMPLEMENTED` n'est
> écrit sans une commande exécutée citée dans ce document ou dans
> [`DSP-RT-EVIDENCE.md`](../qualification/DSP-RT-EVIDENCE.md).

### Provenance des ancres et stabilité

Les mesures ont été prises le **2026-11-11 entre 11:32 et 11:48** sur un arbre
**partagé et activement modifié par d'autres agents** : `git status` montrait
`crates/ravum-lang-codegen/src/emit.rs`, `crates/ravum-lang-sema/src/{own,typeck}.rs`
et `crates/ravum-lang-cli/src/commands/{run,test}.rs` modifiés pendant la session.
Deux binaires ont servi : celui de `5d88ecb` (01:22) pour les premières sondes, et
celui reconstruit à **11:41:51** (5 216 768 octets) ensuite. Les sondes de
capacités et le corpus `effects/` ont été **rejoués sur le second binaire** et
donnent des codes identiques — les affirmations comportementales de ce document
ne dépendent donc pas de l'état transitoire de l'arbre. En revanche les **numéros
de ligne** cités peuvent glisser : ils sont valides au SHA `5d88ecb` ; quand le
fichier est sous édition concurrente, le nom du symbole (fonction, `enum`,
constante) est la référence à utiliser, pas le numéro.

## 1. Ce que « runtime » signifie réellement dans ce dépôt

Le mot « runtime » n'a **pas** ici le sens d'une bibliothèque liée au binaire.
Le runtime Ravum Core 1.0 est un **prélude de code source**, inséré verbatim dans
chaque fichier généré, compilé avec le programme :

| Fait | Preuve |
|---|---|
| Le prélude est une constante Rust `PRELUDE` embarquée dans le générateur | `crates/ravum-lang-codegen/src/prelude.rs:20` |
| Il est inclus dans **chaque** fichier généré (pas de bibliothèque runtime séparée) | ABI-4.3 (`spec/language/abi.md`) |
| Taille mesurée du fichier généré | `examples/hello/.ravum/gen/main.rs` = **7 313 octets** sur disque : ≈ **6 990** octets de bannière + prélude runtime, **326** octets de code utilisateur (mesuré par décalage du premier marqueur `// rv:`) |
| Aides runtime exposées | **24** fonctions `rv_*` (`rv_println`, `rv_fs_read_to_string`, `rv_test`, …) |
| Le programme généré n'utilise **aucun crate externe** | argv `rustc` mesuré (ci-dessous) ; pas de `--extern`, pas de `-L` |
| Le programme généré interdit `unsafe` | `#![forbid(unsafe_code)]` en tête du fichier généré ; seul le wrapper FFI porte `#[allow(unsafe_code)]` (`emit.rs`, `emit_extern_block`) |
| Pas de GC, pas d'ordonnanceur, pas de pool de threads, pas d'allocateur propre | le prélude n'expose que des fonctions séquentielles ; l'allocation est celle de Rust (`String`/`Vec`) |

Invocation `rustc` **capturée** (wrapper `RAVUM_RUSTC` qui journalise ses
arguments, puis délègue à `rustc`) sur `examples/dsp-gain-iir` :

```
--edition 2021 -C "overflow-checks=on" -C "debuginfo=2" -C "opt-level=0"
--crate-name dsp_gain_iir -o <…>\.ravum\bin\dsp_gain_iir.exe <…>\.ravum\gen\main.rs
```

Conséquences de cette forme, qui sont la base de P2-A :

1. **Aucun runtime monolithique obligatoire** : un programme qui n'appelle pas
   `fs` n'embarque pas de code `fs` (le prélude est du code mort éliminé par
   LLVM ; la taille du prélude ne dépend pas des modules utilisés).
2. **Ajouter une capacité = ajouter du code source au prélude**, pas une
   dépendance de lien — sauf si la capacité exige un symbole natif absent du
   jeu de liens par défaut (voir
   [`RFC-P2-VERTICALS-BLOCKERS.md`](RFC-P2-VERTICALS-BLOCKERS.md) §3).
3. **Le coût d'une capacité est visible** : elle apparaît comme fonction `rv_*`
   du prélude et comme entrée de la table des builtins, donc auditable par
   `--emit-rust` (ABI-3.3).

## 2. Modèle d'effets et de profils, tel qu'implémenté

| Élément | Où | État |
|---|---|---|
| 10 variantes d'effet (`io fs net alloc panic unsafe ffi async rt nondet`) | `crates/ravum-lang-sema/src/facts.rs:15` | `IMPLEMENTED` |
| ⚠ Écart de rédaction : EFF-1.1 dit « sous-ensemble des **9** groupes », le tableau de `effects.md` §1 et `facts.rs:15` en énumèrent **10** | `spec/language/effects.md:10,34` | défaut de documentation (code = 10 ; aucun comportement dépend du nombre) |
| `is_unsupported()` = vrai pour `async` et `net` | `facts.rs:61` | `IMPLEMENTED` |
| Profils `core / pure / rt` et leurs ensembles interdits | `facts.rs:116-170` | `IMPLEMENTED` |
| Inférence bottom-up + point fixe sur le graphe d'appels statique | `crates/ravum-lang-sema/src/effects.rs:39` | `IMPLEMENTED` |
| `E-EFF-UNDECLARED` (déclaration insuffisante) | `effects.rs:93-113` | `IMPLEMENTED` |
| `E-EFF-PROFILE` (effet interdit atteint depuis l'entrée) | `effects.rs:143-170` | `IMPLEMENTED` |
| `E-UNSUPPORTED-EFFECT` (déclarer `net`/`async`) | `crates/ravum-lang-sema/src/resolve.rs:1805` | `IMPLEMENTED` |
| Effets exposés aux outils (`facts.effects`, `check --json`) | `facts.rs:330`, `pipeline.rs:164` | `IMPLEMENTED` |

La vérification de profil est **contextuelle à l'entrée** (EFF-4.3) : sous `rt`,
le checker part de `main` (ou de chaque `test fn` s'il n'y a pas de `main`) et
cherche la première fonction qui produit directement un effet interdit
(`effects.rs:377`). Elle ne vérifie **pas** toutes les fonctions du paquet
(question ouverte Q-EFF-6).

Mesure de référence sur l'exemple DSP (`check examples/dsp-gain-iir --json`) :

```
crate::gain    = [alloc]
crate::iir     = [alloc]
crate::main    = [io, alloc]
```

sous `--profile rt` → 1 erreur `E-EFF-PROFILE` — « profile `rt` forbids effect `io`
reachable from `main` » (notes : « call chain: main », « offending call: call to
`std::io::println` »), exit 1. L'exemple DSP ne passe donc **pas** `rt` tel
qu'écrit : son `main` imprime. C'est un fait, pas un défaut — mais il interdit de
présenter l'exemple comme « vérifié rt ».

## 3. Matrice des capacités

Dix capacités demandées par P2-A. Colonnes : statut, où c'est implémenté, nom
d'effet, profils qui l'autorisent, ce qui manque.

| Capacité | Statut | Implémentation | Effet | `core` | `pure` | `rt` | Ce qui manque |
|---|---|---|---|---|---|---|---|
| **`io`** | `IMPLEMENTED` | prélude `rv_println/print/eprintln/eprint/read_line` (`prelude.rs:128-160`) ; table `BUILTIN_FNS` (`builtins.rs`, `static BUILTIN_FNS`) ; émission `FreeBuiltin` (`emit.rs`, `enum FreeBuiltin`) | `io` | oui | **non** (EFF-4.4) | **non** | non-blocage, timeouts, `flush` explicite (ABSENT, `stdlib.md` §14) |
| **`fs`** | `IMPLEMENTED` | prélude `rv_fs_read_to_string/write_string/exists` (`prelude.rs:176-198`) ; `BUILTIN_FNS` | `fs` | oui | **non** | **non** | `File`, `BufReader`, écriture atomique/durable, `try_exists` (ABSENT) ; aucune restriction de chemin à l'exécution (EFF-6.2, **mesuré** : `fs::exists("../../../Cargo.toml")` passe) |
| **`env`** | `IMPLEMENTED` | `rv_env_var` (`prelude.rs:166`), `std::env::var` | `io` + `alloc` (Q-EFF-5 : pas de groupe propre) | oui | **non** | **non** | aucune écriture (`env::set_var` ABSENT), pas de groupe `env` distinct |
| **`args`** | `IMPLEMENTED` | `rv_args` (`prelude.rs:162`), `std::env::args` | `io` + `alloc` | oui | **non** | **non** | pas de module `args` séparé : c'est `env::args` ; pas d'accès indexé typé |
| **`net`** | `REFUSED` | **aucune** : ni module (`BUILTIN_MODULES`, `builtins.rs:83`), ni builtin, ni fonction de prélude | `net` existe mais `is_unsupported()` → déclarer = `E-UNSUPPORTED-EFFECT` (**mesuré**) | — | — | — | tout : groupe d'effet actif, module, builtins, prélude, tests. Voir RFC-P2-B |
| **`http`** | `ABSENT` | rien, nulle part | **aucun groupe n'existe** : `effects(http)` = `E-EFF-UNKNOWN` (EFF-1.2) | — | — | — | groupe d'effet **ou** rattachement explicite à `net`, module, sérialisation, TLS. Voir RFC-P2-C |
| **`sqlite`** | `ABSENT` | rien dans le langage. **Le dépôt sait le faire en Rust** : `ravum-store` (`rusqlite 0.41`, `features=["bundled"]`) — 6 tests passent (`cargo test -p ravum-store`, 2026-11-11) | aucun | — | — | — | groupe d'effet, bindings, **et** capacité de lier du C (RFC-P2-D). Voir RFC-P2-D |
| **`ffi`** | `IMPLEMENTED` (expérimental par D7) | `extern "C" { fn f(…); }` + `unsafe { … }` ; effets `ffi`+`unsafe` inférés (`effects.rs:220-237`) ; émission `emit_extern_block` (`emit.rs`, `emit_extern_block`) ; `#[link_name]` | `ffi` (+ `unsafe`) | oui | **non** | **non** | signatures **primitives seulement** (`E-FFI-TYPE`) : ni pointeur, ni `&[u8]`, ni struct → **impossible de transférer un tampon** ; aucun `#[link]` (RFC-P2-VERTICALS-BLOCKERS §3) |
| **`threads`** | `REFUSED` | **aucune** ; `use std::thread` → `E-RES-UNKNOWN` (**mesuré**) ; CONC-1.1/1.2 : mono-thread `MUST` | **aucun groupe** (`async`/`rt` ne sont pas des mécanismes d'exécution, CONC-1.3) | — | — | — | groupe d'effet, modèle de concurrence, runtime. Décision D6 |
| **`time`** | `ABSENT` | **aucune** ; `time::now()` → `E-RES-UNKNOWN` (**mesuré**) ; `Instant`/`Duration`/`SystemTime`/`sleep` listés ABSENTS (`stdlib.md` §14) | **aucun groupe** ; `nondet` est le candidat naturel mais il est `DECLARED_ONLY` (Q-EFF-3) | — | — | — | au minimum une horloge monotone + un groupe d'effet ; décision normative |

> **`DECLARED_ONLY`** (aucun cas d'usage aujourd'hui) : `nondet` et `rt` sont
> déclarables mais **jamais inférés** et ne prouvent rien (EFF-1.1, Q-EFF-3).
> Un programme qui écrit `effects(rt)` n'est pas plus temps réel qu'un autre.

### 3.1 Trois précisions qui évitent des revendications fausses

- **`unsafe` n'est pas une capacité** : c'est un effet *inféré* par un bloc
  `unsafe { … }`. Il est refusé par `pure` et `rt`, mais il est accepté sous
  `core` — y compris dans un programme qui ne fait rien d'utile.
- **`alloc` est une sur-approximation à granularité fonction** (EFF-2.5) : une
  fonction marquée `alloc` peut ne jamais allouer sur un chemin donné. `alloc`
  est donc un *majorant statique*, pas une mesure — mesurer l'allocation réelle
  exige l'allocateur compteur de `ravum-dsp-harness` (§4).
- **`io` et `fs` sont bloquants** par construction, et le modèle d'effets ne
  distingue **pas** bloquant/non-bloquant, ni latence, ni annulation : il n'y a
  ni `select`, ni timeout, ni `recv_timeout` (ABSENT, `unsupported.md` §5). Le
  profil `rt` les interdit en bloc, ce qui est correct mais **grossier**.

## 4. Ce qui est réellement observable (et ce qui ne l'est pas)

| Observation | Mécanisme | Statut |
|---|---|---|
| Effets inférés par fonction | `check --json` → `summary.effects[].effects` (`pipeline.rs:164-219`) | `IMPLEMENTED` — mesuré |
| Effets déclarés vs inférés (écart) | même JSON, champ `declared` | `IMPLEMENTED` — mesuré (`declared: null` = ensemble inféré public) |
| Refus de profil, avec chaîne d'appel et appel fautif | `E-EFF-PROFILE` + 2 notes | `IMPLEMENTED` — mesuré |
| Allocation réelle pendant une fenêtre | `CountingAllocator` de `ravum-dsp-harness` (`src/alloc.rs`), compteurs par thread | `IMPLEMENTED` — mesuré (0 allocation sur le chemin audio, voir DSP-RT-EVIDENCE) |
| Blocage réel d'un appel | **aucun instrument** | `ABSENT` |
| Timeouts / annulation | **aucune API** | `ABSENT` |
| Taille de pile, bornes d'itération | **aucune** analyse (le profil `rt` ne les vérifie pas) | `ABSENT` — le profil `rt` ne certifie **pas** RT-bounded |
| Panics intrinsèques (débordement, division par zéro, index) | **non suivis** comme effet `panic` (EFF-6.4) | `ABSENT` — approximation documentée |

## 5. Modèle de capacités proposé (P2-A, sans changement normatif)

Le modèle ci-dessous **n'ajoute rien** au langage : il range ce qui existe en
paliers, de sorte qu'un programme puisse déclarer ce à quoi il s'engage, et qu'un
rapport puisse le vérifier. Chaque palier est exprimable **aujourd'hui** avec les
profils et les effets existants.

| Palier | Contenu | Comment le vérifier aujourd'hui | Statut |
|---|---|---|---|
| **T0 — noyau pur** | scalaires, tuples, tableaux, `struct`/`enum` `Copy`, `math::*` | `--profile pure` (EFF-4.2/4.4) | `IMPLEMENTED` — 16/16 cas de conformance `effects/` PASS, dont `pure-main-ok` |
| **T1 — texte alloué** | `String`, `Vec`, conversions, `Error` | `--profile core` + lecture de `facts.effects` (`alloc` présent) | `IMPLEMENTED` |
| **T2 — E/S de l'hôte** | `io`, `fs`, `env`, `args` | `--profile core` ; interdit `pure`/`rt` | `IMPLEMENTED` |
| **T3 — frontière non sûre** | `extern "C"`, `unsafe`, `ffi` | `core` seulement ; `pure`/`rt` refusent (`E-EFF-PROFILE`) | `IMPLEMENTED` — **expérimental** (D7) |
| **TX — réseau / stockage natif** | `net`, `http`, `sqlite`, `threads`, `time` | **rien à vérifier** : les noms n'existent pas | `REFUSED`/`ABSENT` — voir RFC-P2-EXPERIMENTAL-SURFACE |

Ce que ce rangement **n'est pas** : un système de capacités paramétrées. EFF-1.1
interdit explicitement tout effet personnalisé et toute capacité paramétrée
(`MUST`) ; les paliers sont une lecture outillée des ensembles d'effets existants,
pas une nouvelle syntaxe.

## 6. Écarts entre l'objectif P2-A et l'état mesuré

Chaque écart est classé avec l'échelle imposée par le plan P2 :
`FIXABLE_NOW` / `NEEDS_EXPERIMENT` / `AWAITING_HUMAN_DECISION` /
`EXTERNAL_DEPENDENCY` / `DEFERRED`.

| ID | Écart | Preuve | Classe |
|---|---|---|---|
| RC-1 | Aucune distinction bloquant / non-bloquant dans les effets | pas de variante, pas d'analyse ; `io`/`fs` sont bloquants sans le dire | `AWAITING_HUMAN_DECISION` (changer le modèle d'effets est normatif — EFF-1.1) |
| RC-2 | Aucun timeout, aucune annulation, aucune API de sélection | `unsupported.md` §5 | `AWAITING_HUMAN_DECISION` (ADR-008 non ratifié) |
| RC-3 | `rt` ne vérifie pas toutes les fonctions du paquet | `effects.rs:119-142` vérifie l'entrée ; Q-EFF-6 ouverte | `AWAITING_HUMAN_DECISION` — le code est trivial (changer la racine de la BFS), mais cela **change l'ensemble des programmes refusés** : c'est normatif, pas une correction |
| RC-4 | Aucune borne de pile / d'itérations pour `rt` | aucun code ; `docs/specs/04-temps-reel.md` §« Annotations candidates » = conceptuel | `NEEDS_EXPERIMENT` |
| RC-5 | `alloc` n'est pas mesuré à l'exécution par la chaîne d'outils du langage | pas d'allocateur compteur dans `ravum-lang-cli` ; l'instrument existe dans `ravum-dsp-harness` mais n'est pas branché sur les programmes `.rv` | `FIXABLE_NOW` (brancher le compteur est mécanique ; non fait ici) |
| RC-6 | Aucune horloge, donc aucun budget de temps mesurable depuis `.rv` | `time` ABSENT | `AWAITING_HUMAN_DECISION` |
| RC-7 | Le prélude n'expose pas la liste de ses capacités à l'outillage | liste implicite (code `rv_*` + table `builtins.rs`) ; aucun manifeste runtime | `FIXABLE_NOW` (dériver un manifeste de `BUILTIN_FNS`/`BUILTIN_METHODS`) |
| RC-8 | `clean_baseline`/mesures RT non branchées sur `--profile rt` | le profil est un refus statique seulement ; aucun rapport n'est émis par le CLI | `DEFERRED` (lié à RC-4/RC-6) |

## 7. Ce que ce RFC ne décide pas

- Il **ne décide pas** d'ajouter `net`, `http`, `sqlite`, `threads`, `time` : ce
  serait une modification normative (UNS-9.1) réservée au responsable (D6, D2).
- Il **ne propose pas** de nouveau groupe d'effet : EFF-1.1 en gèle le nombre à
  neuf (plus `async`/`rt` réservés). Toute extension du modèle d'effets est un
  changement de spécification, pas une tâche d'implémentation.
- Il **ne promet pas** que les capacités futures seront sûres : EFF-6.1 rappelle
  qu'il n'y a **aucun bac à sable** ; un programme `core` peut lire n'importe quel
  fichier accessible au processus (mesuré : `fs::exists("../../../Cargo.toml")`
  accepté et exécutable).

## 8. Questions ouvertes (pour le responsable)

- **Q-P2A-1** — Le palier T0 (`pure`) doit-il devenir la base d'une
  « bibliothèque vérifiée » (garantie de non-allocation *mesurée*) ou rester un
  simple refus statique ? Recommandation : refus statique + mesure séparée
  (RC-5), sans changer le profil.
- **Q-P2A-2** — Faut-il un groupe d'effet `time` distinct, ou `time` relève-t-il
  de `nondet` (déjà déclarable, sémantique vide) ? Recommandation : ne rien
  ajouter avant qu'un cas d'usage réel (budget RT mesuré) existe — sinon on
  ajoute un nom qui ne prouve rien (Q-EFF-3).
- **Q-P2A-3** — Le profil `rt` doit-il vérifier tout le paquet (Q-EFF-6) avant
  qu'un vertical DSP soit présenté comme « rt-vérifié » ? Recommandation : oui,
  et le documenter comme condition de promotion d'un futur profil RT.
