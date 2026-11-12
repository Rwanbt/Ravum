# Frontière `unsafe` / FFI de Core 1.0 — contrat proposé (D7)

> **Statut : `PROPOSED` · `PENDING_HUMAN_APPROVAL` (D7) · 2026-11-12.**
> Ce document **ne ratifie rien** et ne modifie ni `spec/language/`, ni
> `crates/`, ni `conformance/`. Il fixe ce que « frontière sûre » doit vouloir
> dire pour Core 1.0, ce que l'implémentation fait réellement au code `2b31d63`,
> et ce qui doit être outillé et refusé avant une ratification.
> Inventaire détaillé, contradictions de spécification et tests proposés :
> [`RFC-D7-FFI-EFFECTS.md`](../rfc/RFC-D7-FFI-EFFECTS.md).

## 1. Ce que « sûr » veut dire ici — et ce que cela ne peut pas vouloir dire

**Garantie de forme, pas de mémoire.** Ravum peut prouver que le code généré
contient **un seul** bloc `unsafe`, dans un wrapper engendré à partir de la
déclaration `extern`, et qu'aucun type non autorisé ne traverse la frontière. Il
ne peut pas prouver que la bibliothèque C appelée respecte son contrat : elle
n'est pas compilée par Ravum, elle n'est pas dans sa spécification, et aucun
test ne peut couvrir toutes ses entrées.

Conséquence à écrire dans la documentation utilisateur, sans adoucissement :
**appeler une fonction C place cette bibliothèque dans la base de confiance du
processus.** Un programme Ravum peut donc être corrompu par du code que Ravum
n'a pas analysé. C'est la phrase la plus importante de D7 : une ratification qui
l'omettrait donnerait un `PASS` de forme sur une sûreté non démontrée.

## 2. Surface réelle, vérifiée au code `2b31d63`

| Élément | État réel | Preuve |
|---|---|---|
| Déclaration `extern "C" { fn f(…) -> …; }` | acceptée, **module uniquement** | `parser.rs` (G-3.10/G-3.11) ; cas `unsupported/extern-fn` |
| Définition `extern "C" fn f() { … }` | refusée `E-UNSUPPORTED-EXTERN` | parser ; cas `unsupported/extern-fn` |
| ABI autre que `"C"` | refusée `E-UNSUPPORTED-ABI` | parser ; `unsupported.md` §3 |
| Types admis | **exactement** `i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 isize usize f32 f64 bool ()` | `resolve.rs:1943-1963` (`ffi_safe`) ; copie défensive côté backend `emit.rs:108-134` (`ast_ffi_safe`) |
| `char`, `String`, `Vec`, `&T`, `&str`, `&mut T`, tuples, tableaux, structs/enums | refusés `E-FFI-TYPE` | `resolve.rs:1889`, `resolve.rs:1909` ; cas `types/ffi-bad-type` |
| Appel `extern` hors `unsafe` | refusé `E-UNSAFE-CONTEXT` | `typeck.rs:2043` ; cas `types/ffi-outside-unsafe` |
| `unsafe { … }` | accepté, transparent pour l'ownership | `own.rs` ; cas `ownership/unsafe-transparent` |
| `unsafe fn`, `*const`/`*mut`, `&raw` | refusés `E-UNSUPPORTED-UNSAFE` | parser ; `unsupported.md` §3 |
| `#[link]`, `#[no_mangle]`, `#[repr]` | refusés `E-UNSUPPORTED-ATTR` | `unsupported.md` §3 |
| Effets | un `extern` porte `{unsafe, ffi}` ; un appel `extern` ajoute `ffi` | `resolve.rs:1930` ; `effects.rs:228` |
| Profil `pure` + FFI | refusé `E-EFF-PROFILE` | `ffi.rs::unsafe_is_forbidden_by_the_pure_profile` ; cas `effects/pure-ffi-profile` |
| Émission | `extern "C"` + un wrapper `#[allow(unsafe_code)]` ; l'entête passe de `#![forbid(unsafe_code)]` à `#![deny(unsafe_code)]` | `emit.rs:1008-1012`, `emit.rs:1256`, `emit.rs:1197` (`emit_extern_block`) |
| Édition de liens | **aucune** : seul `#[link_name]` existe, donc seuls les symboles déjà liés par défaut (CRT/libm) sont atteignables | RFC-D7 §2.4 ; mesure `run/ffi-abs` (`abs` de la CRT) |

**Ce qui n'est pas exprimable aujourd'hui** et qui ne doit pas être sous-entendu
par le mot « FFI » : passer un tampon, une chaîne, une struct, un handle ; faire
un callback ; charger une bibliothèque arbitraire ; garantir une ABI stable.

## 3. Contrat proposé

Les clauses ci-dessous sont des **propositions**, pas des règles ratifiées. Elles
ne changent aucun comportement : elles nomment ce qui est déjà vrai et ce qui
devra être refusé.

| ID | Clause |
|---|---|
| **C-FFI-1 — Types** | La liste admise est **fermée** et identique dans le vérificateur et dans le backend. Toute divergence entre `ffi_safe` et `ast_ffi_safe` est un défaut de sûreté : un test doit construire un `CheckedProgram` à la main et vérifier que le backend refuse. |
| **C-FFI-2 — ABI** | Seul `"C"` est admis. Aucune ABI stable n'est revendiquée : la convention d'appel est celle de la cible. `i128`/`u128` traversent aujourd'hui sans contrat de largeur côté C (MSVC n'a pas `__int128`) : soit un test de largeur les couvre sur chaque cible, soit la spécification les retire — décision humaine, pas correctif d'agent. |
| **C-FFI-3 — Durées de vie et propriété** | Aucune référence ne traverse ; rien de possédé ne traverse. Il n'y a donc rien à libérer **aujourd'hui**. Dès qu'un pointeur sera admis, un tableau propriétaire/libérateur et une règle d'aliasing deviennent obligatoires. |
| **C-FFI-4 — Déroulement et terminaison** | Une panique Ravum ne traverse pas la frontière (`panic=abort` dans les harnais concernés). Une fonction C qui `longjmp`, `abort()`, lève un signal ou termine le processus n'a **aucun** comportement spécifié vis-à-vis du hook de panic, du runner de tests ou du contrat de code de sortie de la CLI. À écrire et à tester. |
| **C-FFI-5 — Callbacks** | Inexprimables (pas de type fonction, pas de pointeur de fonction) : **hors périmètre 1.0**, à dire explicitement. |
| **C-FFI-6 — Threads** | Core 1.0 n'expose ni threads ni `Send`/`Sync`. Une fonction C qui crée des threads est hors du modèle de concurrence du langage ; aucune garantie ne doit être promise. |
| **C-FFI-7 — Effets** | `extern` ⇒ `{unsafe, ffi}` ; une déclaration `effects(...)` doit dominer l'inféré ; la propagation à travers un wrapper non déclaré (EFF-2.9) est **spécifiée mais non testée**. |
| **C-FFI-8 — Profils** | `pure` et `rt` refusent `unsafe`/`ffi`. `pure` est testé ; `rt` est spécifié (EFF-4.7) et **NOT_RUN**. |
| **C-FFI-9 — Liaison** | Pas de `#[link]`, pas de `-l`, pas de chemin de bibliothèque : seuls les symboles déjà présents à l'édition de liens par défaut sont appelables. Le terme « FFI » doit être lu comme « appels scalaires à la CRT/libm ». |
| **C-FFI-10 — TCB** | La bibliothèque C appelée appartient à la TCB du processus. Toute documentation de garantie doit le dire, et un profil `pure`/`rt` ne doit jamais l'admettre. |

## 4. Matrice de refus attendue (codes exacts)

| Programme | Code attendu | État |
|---|---|---|
| `extern "system" { … }` | `E-UNSUPPORTED-ABI` | couvert (parser) |
| `extern "C" fn f() { … }` | `E-UNSUPPORTED-EXTERN` | couvert |
| `extern "C" { fn f() -> String; }` | `E-FFI-TYPE` | couvert (`types/ffi-bad-type`) |
| `extern "C" { fn f() -> &str; }` | `E-FFI-TYPE` | à verrouiller par cas |
| `extern "C" { fn f() -> char; }` | `E-FFI-TYPE` | à verrouiller par cas |
| appel `extern` hors `unsafe` | `E-UNSAFE-CONTEXT` | couvert |
| `unsafe fn`, `*const`, `&raw` | `E-UNSUPPORTED-UNSAFE` | couvert (parser) |
| `#[link]`, `#[no_mangle]` | `E-UNSUPPORTED-ATTR` | couvert (refus global) |
| `self` dans un `extern` | `E-RES-SELF` | couvert (contrat) |
| paramètre dupliqué dans un `extern` | `E-RES-DUPLICATE` | couvert (contrat) |
| FFI sous profil `pure` | `E-EFF-PROFILE` | couvert |
| FFI sous profil `rt` | `E-EFF-PROFILE` | **NOT_RUN** |

## 5. Preuves existantes, et ce qui reste non mesuré

**Vérifié par test ou par exécution réelle** : refus hors `unsafe`, acceptation
dans `unsafe`, refus sous `pure`, refus d'un type non scalaire, `self` et
paramètre dupliqué, `deny` + un seul `allow` local, compilation et exécution
réelles de `abs` (`run/ffi-abs`), transparence de l'`unsafe` pour l'ownership.

**Non mesuré, et donc jamais présenté comme un `PASS`** : FFI sous `rt`,
propagation transitive à travers deux wrappers, `i128`/`u128`/`isize`/`usize`/
`bool` à la frontière, unwinding/`longjmp`/signal, ABI multi-plateforme, toute
fonction C qui touche la mémoire (impossible : aucun pointeur n'est admis).

**Hypothèse assumée** : seuls les symboles déjà liés par défaut sont atteignables
(conséquence de l'absence de `#[link]`).

**Inconnue** : le comportement de la bibliothèque C appelée, par construction
hors TCB.

## 6. Ce qui bloque la ratification D7

1. **Contradictions de spécification** (RFC-D7 §3) : `abi.md` ABI-2.5 et ABI-3.3
   interdisent tout `unsafe`/`extern` généré, alors que G-3.10 et
   `unsupported.md` §3 acceptent `extern "C"` et que l'audit backend **exige** le
   wrapper `unsafe`. ABI-3.1/3.2 refusent au parse ce que G-3.10 accepte. Un
   `MUST` doit céder : c'est une décision humaine, pas un correctif.
2. **Tests manquants** : les neuf cas de RFC-D7 §5 (dont `rt`, propagation
   transitive, `char`, `&i32`, `bool`, largeurs, `extern` dans un `impl`).
3. **Phrase de TCB** dans la documentation utilisateur (§1 et C-FFI-10).
4. **Périmètre** : décider si « FFI scalaire seulement » justifie le mot « FFI »
   dans la communication de Core 1.0.

Tant que ces quatre points sont ouverts, la position recommandée reste
**`OPEN`** et la FFI **expérimentale** — jamais « auditée », « sûre » ou
« prête pour une bibliothèque native ».

## 7. Ce que ce document ne fait pas

- Il ne ratifie pas D7 et ne modifie aucune règle `MUST`.
- Il ne présente aucun `NOT_RUN` comme un succès.
- Il ne prétend pas que la frontière est inutile : il mesure qu'elle est
  **étroite, scalaire et visible**, ce qui est précisément ce qui la rend
  auditable.
