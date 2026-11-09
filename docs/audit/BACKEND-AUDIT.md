# Audit adversarial du backend Rust émis (`ravum-lang-codegen`, backend B)

> **Statut : audit interne de second passage — PAS un audit externe certifié, et
> PAS indépendant de l'auteur.** Le backend a été écrit par le worker `codegen`
> lors d'une itération précédente ; cet audit est *adversarial et vérifié par
> exécution*, mais un audit externe reste nécessaire pour la RC. Cet audit n'a
> modifié aucun fichier `src/` ; ses seuls ajouts sont ce document et
> `crates/ravum-lang-codegen/tests/backend_audit.rs`.
>
> Branche : `feat/lang-core-1.0-rc` · commit `8fd1547` · date 2026-11-09.
> **État testé** : arbre de travail = commit `8fd1547` **+ modifications non
> commitées des autres workers** (comme dans `BORROW-CHECKER-AUDIT`) : support
> FFI (`extern "C"`) réparti sur `ast.rs`, `facts.rs`, `emit.rs` (diff
> `emit.rs` : +155/−6) et commandes CLI en cours. `src/emit.rs` fait ~194 Ko /
> 5 562 lignes. Le support FFI **n'est pas commité à ce SHA** : il est audité
> tel quel, et c'est l'objet des findings F0/F0b/F0c. Re-exécuter cet audit
> après chaque correction.
> Toolchain : `rustc 1.95.0 (59807616e 2026-04-14)` / `cargo 1.95.0`,
> hôte `x86_64-pc-windows-msvc`, Windows 11 (10.0.26100).
> Périmètre de preuve : **lecture du code + 21 programmes générés, compilés par
> `rustc --edition 2021` et exécutés**, plus 29 tests unitaires du crate. Pas de
> preuve formelle, pas de fuzzing, pas de vérification bit-à-bit.
>
> Statuts de preuve utilisés : `TESTED` (exécuté sur cet hôte), `NOT_RUN`,
> `ASSUMED`, `UNKNOWN`. Aucun résultat n'est `PROVED`.

## 1. Périmètre et méthode

Sources lues :

- `crates/ravum-lang-codegen/src/emit.rs` (émetteur : items, expressions,
  littéraux, appels, motifs, chemins, types, entrée `main`/`--test`, FFI) ;
- `crates/ravum-lang-codegen/src/prelude.rs` (`PRELUDE`, runtime embarqué) ;
- `crates/ravum-lang-codegen/src/lib.rs` (contrat public `generate`) ;
- `docs/architecture/RAVUM-CORE-DESIGN.md` §3.2/3.3/3.7/3.8 (sémantique de
  référence : overflow, flottants, drop, panics, sortie) ;
- `docs/audit/BORROW-CHECKER-AUDIT.md` (audit voisin, exclut explicitement le
  codegen et la sûreté du code émis).

Démarche :

1. revue statique des chemins sensibles : choix des attributs de crate,
   émission des littéraux, ordre d'évaluation, `?`, index/slices, `drop`,
   appels builtin, runner de tests, FFI ;
2. écriture de **21 tests adversariaux** dans `tests/backend_audit.rs` : chaque
   test construit un `CheckedProgram` à la main (API publique uniquement),
   génère le Rust, le **compile réellement** avec `rustc` et **exécute** le
   binaire (stdout/stderr/code de sortie comparés) ;
3. pour chaque divergence constatée : le test asserte le comportement
   **observé** et nomme le finding — aucun test ne « fait semblant » ;
4. TSan : recherche de multithreading réel dans le workspace (grep), décision
   d'applicabilité, commande prête, statut honnête.

Commandes exactes :

```sh
cargo test -p ravum-lang-codegen --test backend_audit -- --test-threads=1
cargo test -p ravum-lang-codegen -- --test-threads=1
```

## 2. Ce qui est garanti (et vérifié par exécution)

| # | Constat | Test d'audit | Résultat observé |
|---|---|---|---|
| 1 | Programme sans `extern` : le fichier est `#![forbid(unsafe_code)]`, aucun `unsafe`, pointeur brut, `transmute`, `wrapping_*` | `audit_generated_file_is_safe_no_unsafe_constructs` | `TESTED` — conforme |
| 2 | Déterminisme : même entrée ⇒ texte identique | `audit_generated_output_is_deterministic` | `TESTED` — 2 générations byte-identiques ; marqueurs pilotés par l'option |
| 3 | Faits manquants ⇒ erreur explicite, jamais de devinette | `audit_missing_required_facts_are_errors_not_guesses` | `TESTED` — 4 erreurs : `missing call fact`, `missing method fact`, `missing resolution fact`, `missing resolved type`/`cannot resolve nominal type` |
| 4 | Type de littéral absent ⇒ défauts Rust = défauts Ravum (`i32`/`f64`) | `audit_missing_literal_type_..._match_ravum` | `TESTED` — `let x = 5;`/`let y = 1.5;` compile et rend `5`/`1.5` (chemin fail-open documenté, sans divergence observable sur les défauts) |
| 5 | Overflow entier : panic contrôlé + exit 1 avec `-C overflow-checks=on` | `audit_integer_overflow_panics_only_with_overflow_checks` | `TESTED` — `panic: attempt to add with overflow`, exit 1 |
| 6 | Flottants IEEE-754, pas de piège, `Display` court de Rust | `audit_float_display_negative_zero_nan_infinity` | `TESTED` — `-0`, `0.3333333333333333`, `NaN`, `inf`, `-inf`, `100000000000000000000` ; exit 0 |
| 7 | Ordre d'évaluation arguments/opérandes gauche→droite, court-circuit `&&`/`\|\|` | `audit_evaluation_order_...` | `TESTED` — `1 2 3 5 6 7 -1` (les RHS court-circuités n'impriment rien) |
| 8 | Index hors bornes ⇒ panic + exit 1 | `audit_index_out_of_bounds_panics_at_runtime` | `TESTED` — `panic: index out of bounds…`, exit 1 |
| 9 | Slice hors bornes ⇒ panic + exit 1 | `audit_slice_range_out_of_bounds_panics_at_runtime` | `TESTED` — `panic: range end index 9 out of range for slice of length 3`, exit 1 |
| 10 | `as` suit la sémantique Rust documentée (saturant/tronquant) | `audit_casts_are_rust_defined_and_saturating` | `TESTED` — `3`, `0`, `255`, `i64::MAX`, `44` |
| 11 | `drop(x)` est immédiat ; ordre de drop des portées | `audit_drop_is_immediate_and_scope_order_is_unobservable` | `TESTED` — `before`/`after` ; l'ordre inverse de déclaration est inobservable en Core 1.0 (pas de `Drop` utilisateur) et coïncide avec Rust (§3.7) |
| 12 | Panics : `panic: <message>` sur stderr, exit 1 (assert, panic, `Option::unwrap`, `Result::unwrap`) | `audit_panic_payloads_and_exit_codes` | `TESTED` — 4 payloads exacts, exit 1 à chaque fois |
| 13 | Runner `--test` : format texte, résumé, codes de sortie, `main` non exécuté | `audit_test_runner_format_summary_and_exit_codes`, `audit_test_runner_all_pass_exits_zero` | `TESTED` — sortie exacte (`test X ... ok/FAILED`, `1 passed; 2 failed`), exit 1/0, `USER MAIN` absent |
| 14 | Échappement chaînes/caractères : aller-retour byte-exact | `audit_string_and_char_escaping_round_trips` | `TESTED` — `NUL`, `\u{1}`, guillemet, antislash, `é`, emoji ; comparaison octet par octet |
| 15 | Coercitions aux sites d'appel (`&String→&str`, `&[T;N]→&[T]`) | `audit_coercions_apply_at_call_sites` | `TESTED` — `3` puis `4` |
| 16 | FFI : `extern "C"` réellement lié, wrapper sûr côté Ravum | `audit_ffi_relaxes_forbid_unsafe_and_calls_a_real_c_symbol` | `TESTED` — `#[link_name = "abs"]`, `abs(-9)` ⇒ `9`, exit 0 (voir F0) |

Défenses statiques complémentaires (revue de code, pas de test dédié) :

- le prélude ne contient aucun `unsafe`, pointeur brut, `transmute`, ni
  `wrapping_*` ; il n'ajoute aucune instrumentation qui affaiblirait un contrôle
  (deux tests unitaires du crate le vérifient par grep) ;
- les erreurs d'E/S du prélude conservent le message OS (`Display` de
  `std::io::Error`) ;
- lecteur de ligne : `\n` et `\r\n` retirés ; EOF ⇒ chaîne vide (choix
  documenté dans le prélude, non testé ici) ;
- `rv_args()` = `std::env::args().collect()`, donc `argv[0]` inclus (sémantique
  Rust, documentée) ;
- aucune instrumentation d'effets/profil n'est émise (décision du checker).

## 3. Findings

### F0 — MAJEUR (par conception) : `#![forbid(unsafe_code)]` n'est pas inconditionnel

**Constat (code non commité à `8fd1547`).** `run()` (emit.rs l.907–914) choisit
l'attribut de crate selon
`program_uses_extern()` (l.1039) : s'il existe **au moins un** bloc
`extern "C"`, le fichier porte `#![allow(unsafe_code)]` au lieu de
`#![forbid(unsafe_code)]`, et `emit_extern_block` (l.1056) émet, par fonction
externe, une déclaration `extern "C" { #[link_name = "<symbole>"] fn rvm_ffi_<id>(…); }`
et un wrapper `pub fn rvm_ffi_wrap_<id>(…) { unsafe { rvm_ffi_<id>(…) } }`.

**Preuve.** `audit_ffi_relaxes_forbid_unsafe_and_calls_a_real_c_symbol` :
`#![allow(unsafe_code)]` présent, `#![forbid(unsafe_code)]` absent, exactement
**un** bloc `unsafe { … }` pour une fonction externe, `abs(-9)` ⇒ `9`, exit 0.
`audit_ffi_wrapper_passes_rust_types_verbatim_without_abi_validation` montre que
la simple **déclaration** (même jamais appelée) suffit à relâcher l'attribut.

**Impact.** La revendication « le code émis est toujours `forbid(unsafe_code)` »
est **fausse** à ce commit. Le `unsafe` reste généré (jamais recopié de la
source Ravum) et sa surface est bornée aux wrappers, mais elle est autorisée au
niveau du **fichier entier** : toute régression future introduisant de
l'`unsafe` ailleurs passerait le build sans diagnostic.

**Piste (hors périmètre, `src/` interdit ici).** Utiliser
`#![deny(unsafe_code)]` au niveau crate + `#[allow(unsafe_code)]` sur **chaque**
wrapper (contrairement à `forbid`, `deny` est surchargeable localement) : la
garantie redevient « zéro unsafe hors des N wrappers FFI », vérifiable par grep.
Documenter l'exception dans `RAVUM-CORE-DESIGN.md`.

### F0b — MAJEUR si FFI est dans Core 1.0 : aucune validation ABI

**Constat.** Les types Rust déclarés sont recopiés **verbatim** dans la
déclaration C et dans le wrapper. Un `&str` est un pointeur épais (ptr+len), pas
un `const char *` ; une struct utilisateur n'a pas `#[repr(C)]` ; `char` Rust
fait 4 octets, `bool` 1 octet ≠ `_Bool` C sur certaines ABI. Aucune conversion,
aucun diagnostic.

**Preuve.** `audit_ffi_wrapper_passes_rust_types_verbatim_without_abi_validation` :
la génération produit `fn rvm_ffi_wrap_<id>(text: &str) -> &str` et
`unsafe { rvm_ffi_<id>(text) }`. Le test **n'exécute pas** ce programme :
l'appeler serait un comportement indéfini.

**Impact.** Une déclaration `extern` mal typée compile et peut être appelée
depuis du code Ravum « sûr » : la sûreté repose entièrement sur le contrat
`unsafe`/`ffi` du checker, pas sur le backend. C'est le point de rupture le plus
grave si FFI est exposé en Core 1.0.

**Piste.** Restreindre les signatures `extern` à une liste blanche de types
ABI-stables (entiers, `f32`/`f64`, pointeurs explicites si un jour exposés),
émettre des conversions explicites (`*const c_char` via `CString`, `#[repr(C)]`
pour les structs) ou refuser le reste par `CodegenError` nommée.

### F0c — MOYEN : écart spec ↔ implémentation (`unsafe`/`extern`/`ffi` documentés UNSUPPORTED)

**Constat.** `RAVUM-CORE-DESIGN.md` §3.1, §3.6 et §5 déclarent `unsafe` et
`extern` comme mots-clés réservés rejetés (`E-UNSUPPORTED-*`) et les effets
`unsafe`/`ffi` non implémentés en Core 1.0. Le code actuel du checker + backend
accepte `extern "C"` et les blocs Ravum `unsafe { … }` autour d'un appel
externe (le test `e2e_extern_ffi_compiles_and_runs` de `emit.rs` utilise
`let value = unsafe { abs(-9) };`).

**Impact.** La documentation de référence (et donc un consommateur qui applique
la spec) est en désaccord avec le binaire. À trancher avant RC : mise à jour de
la spec et des gates (T6/T9) ou retrait/gating de la fonctionnalité.

### F1 — MOYEN : les overflow-checks ne sont pas fixées par le fichier émis

**Constat.** Le backend n'émet ni `wrapping_*` ni instrumentation ; il dépend de
`-C overflow-checks=on`. Sans ce drapeau (p. ex. `rustc -O`, profil release),
Rust **enveloppe silencieusement**.

**Preuve.** Même fichier généré compilé deux fois :
`-C overflow-checks=on` ⇒ `panic: attempt to add with overflow`, exit 1 ;
`-C overflow-checks=off` ⇒ `-2147483648`, exit 0. Le programme utilise un
opérande runtime (`args().len()`) pour éviter le pliage constant (cf. F2).

**Impact.** La sémantique dépend de la commande de build, pas du fichier émis.
C'est conforme à la décision M5 (le CLI compile avec le drapeau) mais fragile :
aucun mécanisme dans le backend ne détecte un build sans checks.

**Piste.** Garder le drapeau obligatoire dans le CLI (contrat documenté) et
ajouter un test d'intégration CLI qui échoue si le drapeau disparaît.

### F2 — MOYEN-FAIBLE : des panics d'exécution deviennent des erreurs de compilation

**Constat.** Les lints deny-by-default de rustc (`arithmetic_overflow`,
`unconditional_panic`) rejettent à la compilation les opérations **prouvables**
en panic. Ravum §3.3 exige un panic à l'exécution.

**Preuve.** `audit_constant_foldable_panics_are_compile_errors` :
`let max: i32 = 2147483647; max + 1` ⇒ rustc `error: this arithmetic operation
will overflow` ; `a[5]` sur `[i32; 3]` ⇒ `error: this operation will panic at
runtime` — **aucun binaire produit**. Sondes complémentaires (hors suite) :
`5 / 0` et `1 << 32` constants ⇒ même famille d'erreurs ; en revanche la
liste d'allow `unconditional_panic, arithmetic_overflow` fait compiler le
programme, qui panique alors à l'exécution (`index out of bounds…`).

**Impact.** Fail-closed (pas de corruption silencieuse) mais un programme Ravum
valide pour le checker peut ne pas se construire ; l'observateur voit une erreur
rustc, pas un diagnostic Ravum.

**Piste (une ligne).** Ajouter `arithmetic_overflow`, `unconditional_panic` à la
liste `#![allow(...)]` générée (emit.rs l.906), puis vérifier que la suite
`conformance/` reste verte.

### F3 — FAIBLE : l'affectation évalue la valeur avant la place (règle Rust)

**Constat.** Ravum §3.3 annonce « évaluation de gauche à droite » ; pour une
affectation, Rust évalue **le membre droit d'abord**, puis l'expression de place.

**Preuve.** `audit_assignment_evaluates_rhs_before_place` :
`a[idx()] = val()` imprime `val` puis `idx` (puis `9`). Les arguments/opérandes
d'appel et binaires sont bien gauche→droite (test #7).

**Impact.** Observable seulement si la **place** a des effets de bord
(`a[f()] = g()`), cas que le checker n'interdit pas. Aucune corruption mémoire.

**Piste.** Soit documenter l'exception dans la spec (le plus simple), soit
pré-évaluer la place dans un temporaire
(`let __place = f(); a[__place] = g();`) — décision sémantique du Lead.

### F4 — FAIBLE : `exit()` dans un `test fn` contourne le runner

**Constat.** `std::process::exit` n'est pas rattrapable par `catch_unwind` : il
termine le binaire, sans ligne `test …`, sans résumé, et les tests suivants ne
s'exécutent pas.

**Preuve.** `audit_exit_inside_a_test_bypasses_the_runner` : stdout vide,
exit 0, aucun `passed; `.

**Impact.** Un test fautif peut « passer » le runner en le tuant (le code de
sortie est celui du test, pas celui du résumé). Même propriété que le harness
Rust standard avec `process::exit`, mais elle n'est pas documentée côté Ravum.

**Piste.** Documenter ; optionnellement refuser `process::exit` dans un
`test fn` côté checker, ou exécuter chaque test dans un processus séparé (CLI).

### F5 — FAIBLE / `ASSUMED` : `catch_unwind` exige un déroulement (unwind)

**Constat.** Le runner repose sur `catch_unwind`. Compilé avec `-C panic=abort`,
le binaire de test **aborte** au premier test en échec.

**Preuve (sonde hors suite).** Runner minimal compilé `-C panic=abort` :
message de panic par défaut, aucune ligne `FAILED`, aucun résumé, code
`-1073740791` (0xC0000409 sur Windows) au lieu de 1.

**Impact.** Dépendance non exprimée : le CLI doit conserver `panic=unwind` pour
`ravum-lang test`. Le backend ne peut pas l'imposer.

### F6 — INFO : l'allow-list masque `unused_must_use`

`#![allow(unused_must_use, …)]` (emit.rs l.906) supprime l'avertissement Rust
lorsqu'un `Result`/`Option` `#[must_use]` est jeté dans une instruction. Si
Ravum veut interdire ce cas, seul le checker peut le faire ; le backend ne
l'empêche pas et n'émet aucun diagnostic. (Revue de code, pas de test dédié.)

## 4. TSan (ThreadSanitizer)

**Décision : `NOT_APPLICABLE` pour le code émis, `APPLICABLE` pour le workspace
Rust hôte, `NOT_RUN` sur cet hôte.**

1. **Code Ravum émis : `NOT_APPLICABLE`.** Core 1.0 n'a ni threads, ni `async`,
   ni atomiques (mots-clés réservés, `E-UNSUPPORTED-*`) ; `emit.rs` n'émet
   aucun `std::thread`, aucun spawn, aucun canal. Le programme généré est
   mono-thread. Preuve grep : la seule occurrence de `thread` dans le code émis
   est `thread_local!` du prélude (capture du message de panic pour le runner de
   tests) — stockage par thread, pas une construction concurrente.
2. **Workspace hôte : du multithreading réel existe** (preuve grep sur
   `crates/**/tests/*.rs`) :

   | Crate / test | Occurrences |
   |---|---|
   | `ravum-dsp-harness/tests/dsp_stress.rs` | 8 (`thread::spawn` + `thread::scope`) |
   | `ravum-patch-host/tests/rt_lifecycle.rs` | 5 (`thread::scope`) |
   | `ravum-patch-host/tests/crossfade.rs`, `block_host.rs` | 1 + 1 |
   | `ravum-daemon/tests/tcp_multi_client.rs`, `auto_gc_integration.rs`, `daemon_integration.rs` | 1 + 1 + 1 |
   | `ravum-workspace-coordinator/tests/{overlay,git_publication,cycle_integration}.rs` | 3 |
   | `ravum-cli/tests/m1_patch_loop.rs`, `ravum-lang-cli/tests/cli.rs` | 1 + 1 |
   | `ravum-batch`, `ravum-unit-graph` (src) | 1 + 1 |

   TSan est donc **pertinent** pour le workspace, mais ce n'est pas le backend :
   ces tests n'exercent pas le code généré.
3. **Pourquoi `NOT_RUN` ici.** TSan exige un toolchain nightly, `-Zbuild-std`
   (composant `rust-src`) et un **environnement d'exécution Linux**. Sur cet
   hôte : Windows 11, rustc stable `1.95.0` ; `nightly-x86_64-pc-windows-msvc`
   (`1.97.0-nightly`) **est** installé avec `rust-src`, donc la
   cross-compilation est possible, mais le binaire instrumenté TSan ne peut pas
   s'exécuter sous Windows ; la sonde `wsl -l -v` n'a pas répondu en 120 s, donc
   aucun runner Linux n'est *prouvé* disponible. Résultat : `NOT_RUN`
   (et non « vert »).

**Commande prête pour un runner Linux (nightly + `rust-src`) :**

```sh
rustup toolchain install nightly --component rust-src
RUSTFLAGS="-Zsanitizer=thread" cargo +nightly test -p ravum-dsp-harness \
  -Zbuild-std --target x86_64-unknown-linux-gnu --test dsp_stress \
  -- --test-threads=1
# variantes : -p ravum-patch-host --test rt_lifecycle
#             -p ravum-daemon    --test tcp_multi_client
#             -p ravum-workspace-coordinator --test overlay
```

Aucun faux test de concurrence n'a été ajouté pour « faire passer » TSan.

## 5. Verdict

> **Mise à jour (Lead, 2026-11-09)** : les findings F0, F0b, F0c et F2 ont été
> **corrigés après cet audit** (voir §8) et les tests d'audit correspondants ont
> été réécrits pour vérifier le comportement corrigé. Le verdict ci-dessous
> décrit l'état **audité** (avant correctifs) et reste conservé pour la trace.

**PARTIEL — le backend est utilisable pour Core 1.0 sans `extern`, mais la
garantie « safe Rust only » n'est pas tenue dans l'état actuel (F0), et FFI
n'est pas défendable en l'état (F0b/F0c).**

- Pour un programme **sans `extern`** : le fichier est `forbid(unsafe_code)`,
  déterministe, sans construction Rust dangereuse ; les chemins panique/exit/
  runner/échappement/coercitions/ordre d'évaluation testés par exécution réelle
  sont conformes ; les faits manquants produisent des erreurs explicites. Les
  divergences restantes (F1–F5) sont bornées, documentées par des tests, et
  aucune ne produit de corruption silencieuse sur la surface testée.
- **Bloquants pour une RC « backend sûr »** : F0 (relâchement global de
  `forbid(unsafe_code)` dès qu'un `extern` est déclaré), F0b (aucune validation
  ABI), F0c (spec déclarant FFI/`unsafe` non supportés alors qu'ils sont
  émis). Ces trois points exigent une décision explicite (Lead/ADR), pas un
  correctif cosmétique.
- **À corriger/documenter avant RC** : F2 (une ligne d'allow-list, sinon des
  programmes valides ne se construisent pas), F1/F5 (contrat de build du CLI),
  F3/F4 (spec/diagnostic).

## 6. Résultats exacts

```text
$ cargo test -p ravum-lang-codegen -- --test-threads=1
running 29 tests             ... test result: ok. 29 passed; 0 failed; 0 ignored
running 21 tests             ... test result: ok. 21 passed; 0 failed; 0 ignored
running 0 tests (doctests)   ... ok

# total : 50 tests verts (29 unitaires du crate + 21 tests d'audit)
# côté audit : 19 invocations réelles de rustc — 16 compilations-exécutions via
# `must_compile` + 3 appels directs (1 avec overflow-checks=off exécuté, 2 rejets
# volontairement attendus) ; 17 binaires produits et exécutés, 2 compilations
# refusées comme prévu (F2).
```

## 7. Limites de l'audit

- **Auto-audit adversarial, non indépendant de l'auteur** : la relecture
  critique et l'exécution sont réelles, mais un audit externe reste requis.
- **Pas de preuve formelle** ni de vérification bit-à-bit de l'équivalence
  Ravum ↔ Rust ; les affirmations sont comportementales, sur les points listés.
- 21 programmes écrits à la main ; **pas de fuzzing**, pas de génération
  aléatoire d'AST, pas d'exploration exhaustive des combinaisons.
- Comportement vérifié uniquement sur **Windows 11 / `rustc 1.95.0`** ; les
  messages de panic natifs et les lints peuvent changer avec la version de
  rustc. Aucune exécution Linux, aucune ABI non-MSVC testée (le seul symbole C
  exercé est `abs` de la CRT).
- **TSan `NOT_RUN`** (voir §4) ; aucun résultat de concurrence n'est établi.
- Non couverts : génériques/monomorphisation, récursion profonde/stack overflow,
  OOM, E/S réelles du prélude (messages OS), `rv_read_line` en EOF, variables
  d'environnement, effets/profils (non émis), interactions avec les trous du
  checker de possession (`BORROW-CHECKER-AUDIT` F2) — notons que ces derniers
  produisent du Rust que le borrow checker de rustc refuse (échec visible), pas
  du code silencieusement faux, mais ce filet n'est pas audité ici.
- Le périmètre exclut `src/` : **aucun correctif n'est appliqué**, seulement des
  repros et des pistes.

## 8. Correctifs appliqués après l'audit (Lead)

| Finding | Correctif | Vérification |
|---|---|---|
| F0 | `#![deny(unsafe_code)]` au niveau crate pour les programmes FFI, `#[allow(unsafe_code)]` **uniquement** sur chaque wrapper généré ; les programmes sans `extern` conservent `#![forbid(unsafe_code)]`. | `e2e_extern_ffi_compiles_and_runs` et `audit_ffi_relaxes_forbid_unsafe_and_calls_a_real_c_symbol` (deny + exactement un allow local) |
| F0b | Le backend valide les types `extern` sur l'AST (`ast_ffi_safe`) et retourne `CodegenError` si un paramètre/retour n'est pas un entier, `f32`/`f64`, `bool` ou `()` — même si le `CheckedProgram` est construit à la main. | `audit_ffi_rejects_non_ffi_safe_types` |
| F2 | Ajout de `arithmetic_overflow, unconditional_panic` à l'allow-list générée : les panics prouvables compilent et paniquent à l'exécution (conforme §3.3). | `audit_constant_foldable_panics_are_runtime_panics` (overflow ⇒ exit 1 ; index OOB ⇒ exit 1) |
| F0c | Spec/design mis à jour : `unsafe`/`extern`/`ffi` documentés comme supportés dans le cadre FFI-1..7 ; `UNSUPPORTED` réduit aux pointeurs bruts, `unsafe fn`, `extern fn` avec corps, ABI non-C, types composés. | `spec/language/{grammar,effects,unsupported,features,diagnostics}.md`, `RAVUM-CORE-DESIGN.md` §3.7 bis |
| F1 | Contrat conservé : le CLI compile toujours avec `-C overflow-checks=on` ; documenté, à couvrir par un test CLI dédié. | `docs/architecture/RAVUM-CORE-DESIGN.md`, `BACKEND-AUDIT` F1 |
| F3 | Exception documentée : l'affectation évalue la valeur avant la place (règle Rust), contrairement à l'énoncé « gauche→droite » ; à trancher en ADR si un jour observable. | §3.3 du design (note) |
| F4/F5/F6 | Documentés (`exit` dans un test, `panic=abort` interdit pour `test`, `unused_must_use` masqué) ; aucun changement de code. | `docs/CLI-WATCH-WASM.md`, `BACKEND-AUDIT` |

Verdict après correctifs : **PARTIAL+** — « safe Rust only » redevient
vérifiable (zéro `unsafe` hors wrappers FFI), l'ABI est refusée au backend si
elle n'est pas primitive, et les panics prouvables sont exécutables. Restent
non couverts : preuve formelle, audit externe, TSan (NOT_RUN), et F1/F3–F6
comme limites/documentations.
