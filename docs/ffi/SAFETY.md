# Frontière `unsafe` / FFI de Core 1.0 — contrat de la frontière scalaire

> **Statut : APPROUVÉ par le responsable, 2026-11-14.**
> Les clauses C-FFI-1 à C-FFI-10 engagent la frontière scalaire de Core 1.0 :
> `extern "C"` avec des types scalaires admis, appelable uniquement dans
> `unsafe`. Cette décision (S4) **retire `i128` et `u128`** des types admis et
> place **hors contrat** une fonction C qui déroule, `longjmp`, lève un signal
> ou termine le processus (C-FFI-4 : obligation de l'appelant, non vérifiée).
> Elle ne ratifie ni `D2`, ni `D4`, ni `T12` : aucun audit externe n'a couvert
> la frontière, et aucun document ne peut écrire `PROVED` ni « production
> ready ».
>
> État de l'implémentation sur cette branche : le vérificateur
> (`crates/ravum-lang-sema/src/resolve.rs`, `ffi_safe`) applique la liste
> approuvée ; la liste jumelle du backend
> (`crates/ravum-lang-codegen/src/emit.rs`, `ast_ffi_safe`) admet encore
> `i128`/`u128` et doit être alignée (agent E, vague 2). C-FFI-1 reste une
> exigence : cette divergence est un défaut de sûreté **tracé**, et le test
> C-FFI-1 prouve que le vérificateur refuse avant le backend.
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

## 2. Surface réelle (vérifiée sur le worktree de la vague 3, 2026-11-14)

| Élément | État réel | Preuve |
|---|---|---|
| Déclaration `extern "C" { fn f(…) -> …; }` | acceptée, **module uniquement** | `parser.rs` (G-3.10/G-3.11) ; cas `unsupported/extern-fn` |
| Définition `extern "C" fn f() { … }` | refusée `E-UNSUPPORTED-EXTERN` | parser ; cas `unsupported/extern-fn` |
| ABI autre que `"C"` | refusée `E-UNSUPPORTED-ABI` | parser ; `unsupported.md` §3 |
| Types admis | **exactement** `i8 i16 i32 i64 u8 u16 u32 u64 isize usize f32 f64 bool ()` ; `i128`/`u128` **retirés** (S4, 2026-11-14) et refusés `E-FFI-TYPE` | `resolve.rs` (`ffi_safe`) ; copie défensive côté backend `emit.rs` (`ast_ffi_safe`), **encore à aligner** (agent E) |
| `char`, `i128`, `u128`, `String`, `Vec`, `&T`, `&str`, `&mut T`, tuples, tableaux, structs/enums | refusés `E-FFI-TYPE` | `resolve.rs` (`resolve_extern_block`) ; cas `types/ffi-bad-type`, `types/ffi-i128-param`, `types/ffi-u128-return` |
| Appel `extern` hors `unsafe` | refusé `E-UNSAFE-CONTEXT` | `typeck.rs` (`E-UNSAFE-CONTEXT`) ; cas `types/ffi-outside-unsafe` |
| `unsafe { … }` | accepté, transparent pour l'ownership | `own.rs` ; cas `ownership/unsafe-transparent` |
| `unsafe fn`, `*const`/`*mut`, `&raw` | refusés `E-UNSUPPORTED-UNSAFE` | parser ; `unsupported.md` §3 |
| `#[link]`, `#[no_mangle]`, `#[repr]` | refusés `E-UNSUPPORTED-ATTR` | `unsupported.md` §3 |
| Effets | un `extern` porte `{unsafe, ffi}` ; un appel `extern` ajoute `ffi` | `resolve.rs` (`resolve_extern_block`) ; `effects.rs` (EFF-2.9) |
| Confinement | `#![forbid(unsafe_code)]` sur `syntax`, `sema`, `codegen`, `lsp` ; le CLI porte `#![deny(unsafe_code)]` et **deux** `#[allow(unsafe_code)]` seulement, sur `commands::watch::signal` | inventaire : [`UNSAFE-INVENTORY.md`](UNSAFE-INVENTORY.md) ; vérifié par `tools/unsafe_audit.py` (CI `gate-contracts`) |
| Profil `pure` + FFI | refusé `E-EFF-PROFILE` | `ffi.rs::unsafe_is_forbidden_by_the_pure_profile` ; cas `effects/pure-ffi-profile` |
| Émission | `extern "C"` + un wrapper `#[allow(unsafe_code)]` ; l'entête passe de `#![forbid(unsafe_code)]` à `#![deny(unsafe_code)]` | `emit.rs` (`emit_extern_block`) |
| Édition de liens | **aucune** : seul `#[link_name]` existe, donc seuls les symboles déjà liés par défaut (CRT/libm) sont atteignables | RFC-D7 §2.4 ; mesure `run/ffi-abs` (`abs` de la CRT) |

**Ce qui n'est pas exprimable aujourd'hui** et qui ne doit pas être sous-entendu
par le mot « FFI » : passer un tampon, une chaîne, une struct, un handle ; faire
un callback ; charger une bibliothèque arbitraire ; garantir une ABI stable.

## 3. Contrat approuvé

Les clauses ci-dessous sont **APPROUVÉES par le responsable, 2026-11-14**
(décision S4). Elles nomment ce qui est vrai et ce qui doit être refusé à la
frontière scalaire ; elles ne prétendent pas à une sûreté mémoire de la
bibliothèque C appelée.

| ID | Clause |
|---|---|
| **C-FFI-1 — Types** | La liste admise est **fermée** : `i8 i16 i32 i64`, `u8 u16 u32 u64`, `isize usize`, `f32 f64`, `bool`, `()`. `i128`/`u128` en sont **retirés** (S4, 2026-11-14 ; C-FFI-2). La liste doit être **identique** dans le vérificateur (`ffi_safe`) et dans le backend (`ast_ffi_safe`) ; toute divergence est un défaut de sûreté. Le vérificateur refuse `i128`/`u128` **avant** le backend (test `c_ffi_1_verifier_refuses_i128_and_u128_before_the_backend`, et cas `types/ffi-i128-param` / `types/ffi-u128-return`). Quand l'agent E aligne `ast_ffi_safe`, un test doit en plus construire un `CheckedProgram` à la main et vérifier que le backend refuse. |
| **C-FFI-2 — ABI** | Seul `"C"` est admis. Aucune ABI stable n'est revendiquée : la convention d'appel est celle de la cible. `i128`/`u128` sont **retirés** des types admis (S4, 2026-11-14) : aucune largeur C portable ne les couvre (MSVC n'a pas `__int128`). Toute déclaration qui les utilise est refusée `E-FFI-TYPE`. |
| **C-FFI-3 — Durées de vie et propriété** | Aucune référence ne traverse ; rien de possédé ne traverse. Il n'y a donc rien à libérer **aujourd'hui**. Dès qu'un pointeur sera admis, un tableau propriétaire/libérateur et une règle d'aliasing deviennent obligatoires. |
| **C-FFI-4 — Déroulement et terminaison** | Une panique Ravum ne traverse pas la frontière (`panic=abort` dans les harnais concernés). Une fonction C qui déroule (exception C++/`longjmp`), `abort()`, lève un signal ou termine le processus est **hors contrat** (S4, 2026-11-14) : c'est une **obligation de l'appelant**, **non vérifiée** par la toolchain, et aucun comportement n'est spécifié vis-à-vis du hook de panic, du runner de tests ou du contrat de code de sortie de la CLI. |
| **C-FFI-5 — Callbacks** | Inexprimables (pas de type fonction, pas de pointeur de fonction) : **hors périmètre 1.0**, à dire explicitement. |
| **C-FFI-6 — Threads** | Core 1.0 n'expose ni threads ni `Send`/`Sync`. Une fonction C qui crée des threads est hors du modèle de concurrence du langage ; aucune garantie ne doit être promise. |
| **C-FFI-7 — Effets** | `extern` ⇒ `{unsafe, ffi}` ; une déclaration `effects(...)` doit dominer l'inféré. La propagation à travers un wrapper non déclaré (EFF-2.9) est **testée depuis le 2026-11-13** (`effects/ffi-transitive-wrapper`, chaîne `main -> outer -> inner`). |
| **C-FFI-8 — Profils** | `pure` et `rt` refusent `unsafe`/`ffi`. Les deux sont **testés** : `effects/pure-ffi-profile` et, depuis le 2026-11-13, `effects/ffi-rt-profile` (EFF-4.7). |
| **C-FFI-9 — Liaison** | Pas de `#[link]`, pas de `-l`, pas de chemin de bibliothèque : seuls les symboles déjà présents à l'édition de liens par défaut sont appelables. Le terme « FFI » doit être lu comme « appels scalaires à la CRT/libm ». |
| **C-FFI-10 — TCB** | La bibliothèque C appelée appartient à la TCB du processus. Toute documentation de garantie doit le dire, et un profil `pure`/`rt` ne doit jamais l'admettre. |

## 4. Matrice de refus attendue (codes exacts)

| Programme | Code attendu | État |
|---|---|---|
| `extern "system" { … }` | `E-UNSUPPORTED-ABI` | couvert (parser) |
| `extern "C" fn f() { … }` | `E-UNSUPPORTED-EXTERN` | couvert |
| `extern "C" { fn f() -> String; }` | `E-FFI-TYPE` | couvert (`types/ffi-bad-type`) |
| `extern "C" { fn f() -> &str; }` | `E-FFI-TYPE` | couvert (`types/ffi-str-rejected`, 2026-11-14) |
| `extern "C" { fn f() -> char; }` | `E-FFI-TYPE` | couvert (`types/ffi-char-rejected`, 2026-11-13) |
| `extern "C" { fn f(x: i128) -> i32; }` | `E-FFI-TYPE` | couvert (`types/ffi-i128-param`, 2026-11-14) |
| `extern "C" { fn f() -> u128; }` | `E-FFI-TYPE` | couvert (`types/ffi-u128-return`, 2026-11-14) |
| `extern "C" { fn f(x: &i32) -> i32; }` | `E-FFI-TYPE` | couvert (`types/ffi-reference-rejected`, 2026-11-13) |
| appel `extern` hors `unsafe` | `E-UNSAFE-CONTEXT` | couvert |
| `unsafe fn`, `*const`, `&raw` | `E-UNSUPPORTED-UNSAFE` | couvert (parser) ; pointeur **dans** `unsafe` couvert (`types/unsafe-does-not-unlock-pointers`) |
| `#[link]`, `#[no_mangle]` | `E-UNSUPPORTED-ATTR` | couvert (refus global) |
| `self` dans un `extern` | `E-RES-SELF` | couvert (contrat) |
| paramètre dupliqué dans un `extern` | `E-RES-DUPLICATE` | couvert (contrat) |
| `extern` dans un `impl` | `E-UNSUPPORTED-EXTERN` (observé) | couvert (`types/ffi-extern-in-impl`, 2026-11-13) |
| FFI sous profil `pure` | `E-EFF-PROFILE` | couvert |
| FFI sous profil `rt` | `E-EFF-PROFILE` | couvert (`effects/ffi-rt-profile`, 2026-11-13) |
| `bool` à la frontière | admission + build | couvert (`types/ffi-bool-boundary`, `compile-pass`) ; **aucune valeur ne traverse** (pas de symbole CRT `_Bool`) |
| `i64` à l'exécution | valeur attendue | couvert (`run/ffi-width-boundary`, `llabs(-9000000000) = 9000000000`) |

## 5. Preuves existantes, et ce qui reste non mesuré

**Vérifié par test ou par exécution réelle** : refus hors `unsafe`, acceptation
dans `unsafe`, refus sous `pure` **et** sous `rt`, propagation transitive à
travers deux wrappers, refus d'un type non scalaire (`String`, `char`, `&i32`,
`&str`), refus de `i128`/`u128` (paramètre et retour, `E-FFI-TYPE` au
vérificateur, sans `CheckedProgram` produit), `self` et paramètre dupliqué,
`extern` dans un `impl`, pointeur brut refusé dans un `unsafe`, `deny` + un
seul `allow` local, compilation et exécution réelles de `abs` (`run/ffi-abs`),
passage réel d'un `i64` (`run/ffi-width-boundary`), admission + build d'un
`bool` (`types/ffi-bool-boundary`), transparence de l'`unsafe` pour l'ownership.

**Dette d'implémentation tracée (2026-11-14)** : sur cette branche,
`emit.rs::ast_ffi_safe` admet encore `i128`/`u128` alors que `resolve.rs::ffi_safe`
ne les admet plus. L'alignement du backend est confié à l'agent E (vague 2).
Tant que la divergence existe, C-FFI-1 est un **défaut de sûreté tracé**, pas un
`PASS` : le vérificateur refuse avant le backend (test C-FFI-1 + deux cas de
conformance), donc aucun programme accepté ne peut les atteindre, mais la liste
jumelle n'est pas encore identique.

**Confinement vérifié mécaniquement (2026-11-14)** : `python tools/unsafe_audit.py`
→ PASS. Les quatre crates du langage portent `#![forbid(unsafe_code)]` ; le CLI
porte `#![deny(unsafe_code)]` et exactement deux `#[allow(unsafe_code)]`, sur
`commands::watch::signal`, avec commentaires `// SAFETY:` sur les six sites.
L'inventaire complet est [`UNSAFE-INVENTORY.md`](UNSAFE-INVENTORY.md) ; le script
tourne dans `gate-contracts.yml` et a 7 tests. Le lint du compilateur rend
l'ajout d'un `unsafe` non listé impossible à compiler dans ces crates.

**Non mesuré, et donc jamais présenté comme un `PASS`** : `isize`/`usize` à
l'exécution (déclarations seules), valeur `bool` traversante, unwinding/`longjmp`/
signal (hors contrat, C-FFI-4), ABI multi-plateforme, toute fonction C qui touche
la mémoire (impossible : aucun pointeur n'est admis). La largeur C de
`i128`/`u128` n'est plus une question ouverte : ces types sont refusés (S4,
2026-11-14). **Miri sur les cas FFI `NOT_RUN` par construction** :
les cas exécutables appellent la vraie CRT (`abs`, `llabs`), que Miri ne peut pas
interpréter ; Miri couvre les crates du langage hors appels C, pas la frontière.

**Hypothèse assumée** : seuls les symboles déjà liés par défaut sont atteignables
(conséquence de l'absence de `#[link]`).

**Inconnue** : le comportement de la bibliothèque C appelée, par construction
hors TCB.

## 6. Ce qui reste hors de l'approbation (2026-11-14)

Les clauses C-FFI-1 à C-FFI-10 sont **APPROUVÉES par le responsable, 2026-11-14**
(décision S4). Les points suivants ne sont **pas** couverts par cette
approbation et ne doivent jamais être présentés comme des preuves :

1. **Audit externe (`D4`)** : `NOT_RUN`. Aucun audit indépendant n'a couvert la
   frontière C ; aucun document ne peut écrire `PROVED`, « auditée » ou
   « production ready ».
2. **Socle (`D2`)** : les documents `spec/language/` restent `DRAFT —
   CANDIDATE`. L'approbation porte sur les clauses de ce document et sur la
   liste de types FFI admis, pas sur une ratification du socle.
3. **Contradictions de spécification** (RFC-D7 §3) : `abi.md` ABI-2.5 et ABI-3.3
   interdisent tout `unsafe`/`extern` généré, alors que G-3.10 et
   `unsupported.md` §3 acceptent `extern "C"` et que l'audit backend **exige** le
   wrapper `unsafe`. ABI-3.1/3.2 refusent au parse ce que G-3.10 accepte. La
   résolution d'un `MUST` reste une décision humaine.
4. **Liste jumelle du backend** : `emit.rs::ast_ffi_safe` admet encore
   `i128`/`u128` ; l'alignement est confié à l'agent E (vague 2). C'est une
   divergence **tracée**, couverte côté vérificateur (test C-FFI-1), pas un
   `PASS` de C-FFI-1.
5. **Go/No-Go (`T12`)** : non tranché.
6. **`C-FFI-4`** : une fonction C qui déroule, `longjmp`, lève un signal ou
   termine le processus est **hors contrat**, à la charge de l'appelant. Aucun
   test ne peut l'établir pour une bibliothèque hors TCB.

La FFI reste **expérimentale** (au sens de `D6`) : jamais « certifiée mémoire »,
jamais admise par un profil `pure`/`rt`, jamais « prête pour une bibliothèque
native ».

## 7. Ce que ce document ne fait pas

- Il ne ratifie ni `D2`, ni `D4`, ni `T12` ; il enregistre l'approbation S4 des
  clauses C-FFI-1 à C-FFI-10 et de la liste de types admis (2026-11-14).
- Il ne présente aucun `NOT_RUN`, `SKIP`, `PARTIAL` ou `UNKNOWN` comme un succès.
- Il ne prétend pas que la frontière est inutile : il mesure qu'elle est
  **étroite, scalaire et visible**, ce qui est précisément ce qui la rend
  auditable.
