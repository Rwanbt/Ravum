# RFC — D7, frontière `unsafe` / FFI : inventaire mesuré, contrats manquants et tests négatifs

> **Statut : `PROPOSED` · `PENDING_HUMAN_APPROVAL`** · **Date : 2026-11-11**
> **Décision liée : D7** (`DECISIONS_REQUIRED.md`, `CORE-1.0-GATE-MATRIX.md` §4)
> **Ce document ne ratifie rien, ne modifie ni `spec/language/`, ni `crates/`, ni
> `conformance/`.** Il inventorie ce qui existe réellement et propose des tests
> négatifs ; il ne présente aucune conclusion d'audit comme acquise.

## 1. Problème

D7 demande si la frontière `unsafe`/FFI de Core 1.0 peut être ratifiée. La
réponse dépend de ce qui est **implémenté**, de ce qui est **spécifié** et de ce
qui est **testé** — trois ensembles qui ne coïncident pas aujourd'hui. Cette RFC
les mesure séparément et nomme les contrats absents.

Avertissement de lecture, qui n'est pas une clause de style : **appeler une
bibliothèque native à travers un wrapper Rust ne rend pas le programme sûr pour
la mémoire.** Le wrapper `unsafe { … }` généré ne valide rien : il encapsule
l'appel, il ne prouve pas que la fonction C respecte les tailles, les pointeurs
ou les invariants qu'elle prétend respecter. La sûreté mémoire du code Rust
généré est une propriété locale du fichier généré ; le processus complet dépend
de la bibliothèque C, qui n'est pas dans la TCB décrite par `spec/language/`.

## 2. Inventaire mesuré de ce qui est implémenté

### 2.1 Surface syntaxique et sémantique

| Élément | État réel | Preuve (fichier:ligne) |
|---|---|---|
| Déclaration `extern "C" { fn f(i32) -> i32; }` | acceptée, au niveau module uniquement | `crates/ravum-lang-syntax/src/parser.rs:1269-1350` ; `grammar.md` G-3.10/G-3.11 |
| `extern "C" fn f() { … }` (définition avec corps) | refusée `E-UNSUPPORTED-EXTERN` | `parser.rs:1810` ; cas `unsupported/extern-fn` |
| ABI autre que `"C"` (`"system"`, `"Rust"`) | refusée `E-UNSUPPORTED-ABI` | `parser.rs:1290` ; `unsupported.md` §3 |
| `unsafe { … }` en expression | accepté, transparent pour l'ownership | `own.rs:750,1726` ; cas `ownership/unsafe-transparent` |
| `unsafe fn`, pointeurs bruts `*const/*mut`, `&raw` | refusés `E-UNSUPPORTED-UNSAFE` | `parser.rs:1806` ; `unsupported.md` §3 |
| Attributs `#[no_mangle]`, `#[link]`, `#[repr]` | refusés `E-UNSUPPORTED-ATTR` | `unsupported.md` §3 ; aucun chemin d'acceptation |
| Appel d'une fonction `extern` hors `unsafe` | refusé `E-UNSAFE-CONTEXT` | `typeck.rs:1863` ; cas `types/ffi-outside-unsafe` |
| `self` dans une déclaration `extern` | refusé `E-RES-SELF` | `resolve.rs:1867-1873` ; contrat `resolve_contracts.rs` |
| Paramètre `extern` dupliqué | refusé `E-RES-DUPLICATE` | `resolve.rs:1875-1881` ; contrat `resolve_contracts.rs` |

### 2.2 Types admis à la frontière — la vraie limite

`resolve.rs:1939-1959` (`fn ffi_safe`) et sa copie défensive côté backend
`emit.rs:102-126` (`fn ast_ffi_safe`) admettent **exactement** :

```
i8 i16 i32 i64 i128  u8 u16 u32 u64 u128  isize usize  f32 f64  bool  ()
```

Sont refusés `E-FFI-TYPE` : `char`, `String`, `Vec<T>`, `&T`, `&str`, `&mut T`,
les tuples, les tableaux, les structs/enums nominaux, `Option`/`Result`. Le
message du diagnostic est littéralement « integers, floats, `bool` or `()`
(…non-FFI-safe…) » (`resolve.rs:1885-1911`).

Conséquence mesurée par construction : **il n'existe aucun moyen d'échanger un
tampon, une chaîne, une structure ou un handle avec le code natif.** Un `extern`
accepté est une fonction scalaire → scalaire. Le seul cas exécuté du corpus est
`abs(i32) -> i32` (`run/ffi-abs`, `extern "C" { fn abs(x: i32) -> i32; }`,
`stdout_contains: "7"`).

### 2.3 Effets

| Règle effective | Implémentation | Preuve |
|---|---|---|
| Un bloc `unsafe` ajoute l'effet `unsafe` | oui | `effects.rs:303-307` |
| Un appel `extern` ajoute `unsafe` **et** `ffi` | oui | `effects.rs:217-236` |
| Une signature `extern` porte `{unsafe, ffi}` fixes | oui | `resolve.rs:1926-1930` |
| Déclaration `effects(...)` = majorant de l'inféré | oui (`E-EFF-UNDECLARED`) | `effects.md` EFF-3.1/EFF-3.4 ; cas `effects/declaration-insufficient` |
| Profil `pure` : `unsafe`/`ffi` atteignables ⇒ `E-EFF-PROFILE` | oui, testé | `ffi.rs::unsafe_is_forbidden_by_the_pure_profile` ; cas `effects/pure-ffi-profile` |
| Profil `rt` : idem (`EFF-4.7`) | **spécifié, non testé** | `effects.md` EFF-4.7 ; aucun test `rt` + FFI trouvé |
| Effets transitifs à travers un wrapper | l'inférence existe (arêtes d'appel), mais **aucun test** ne vérifie qu'un wrapper non déclaré propage `unsafe`/`ffi` jusqu'au point d'entrée | `effects.rs` (fixpoint) ; trou de couverture |

### 2.4 Ce que le backend émet

`emit.rs:1185-1256` : pour chaque `extern`, un `extern "C" { #[link_name = "…"]
fn rvm_ffi_<id>(…); }` **et** un wrapper public `#[allow(unsafe_code)] pub fn
rvm_ffi_wrap_<id>(…)` contenant `unsafe { rvm_ffi_<id>(…) }`. Si le programme
déclare au moins un `extern`, l'entête passe de `#![forbid(unsafe_code)]` à
`#![deny(unsafe_code)]` (`emit.rs:992-1000`), de sorte que **seul** le wrapper
est autorisé à contenir de l'`unsafe` ; tout autre `unsafe` casserait la
compilation. C'est une garantie de forme, pas de sûreté.

`#[link_name]` lie le nom déclaré au symbole C. **Aucun attribut `#[link(name =
"…")]` n'est possible** (attributs refusés) : seuls les symboles déjà présents
dans l'édition de liens par défaut (libc/libm côté plateforme) sont atteignables.
La mesure `run/ffi-abs` le confirme (symbole `abs` de la CRT) ; `libm`, une
bibliothèque tierce ou un `.dll`/`.so` quelconque **ne sont pas exprimables**
aujourd'hui.

### 2.5 Preuves existantes (inventaire exhaustif au SHA `5d88ecb`)

| Catégorie | Fichier / cas | Ce qui est réellement vérifié |
|---|---|---|
| Tests sema | `crates/ravum-lang-sema/tests/ffi.rs` (5) | refus hors `unsafe` ; acceptation dans `unsafe` ; `pure` refuse ; déclaration `effects(unsafe, ffi)` acceptée ; `String` refusé |
| Contrats resolution | `resolve_contracts.rs` (2) | `self` interdit, paramètre dupliqué |
| Audit codegen | `backend_audit.rs::audit_generated_file_is_safe_no_unsafe_constructs` | `#![forbid(unsafe_code)]` et absence d'`unsafe` **sur un programme sans `extern`** |
| Audit codegen FFI | `backend_audit.rs::audit_ffi_relaxes_forbid_unsafe_and_calls_a_real_c_symbol` | `deny` + un seul `allow` local + un seul bloc `unsafe` + compilation réelle |
| E2E codegen | `emit.rs::e2e_extern_ffi_compiles_and_runs` | `abs(-9)` compilé **et exécuté** |
| Conformance | `run/ffi-abs` | exécution réelle, sortie `7` |
| Conformance | `types/ffi-outside-unsafe`, `types/ffi-bad-type`, `unsupported/extern-fn` | refus + code exact |
| Conformance | `effects/pure-ffi-profile` | `E-EFF-PROFILE` sous `pure` |
| Conformance | `ownership/unsafe-transparent` | l'`unsafe` ne change pas la propriété |

Aucun test ne couvre : `rt` + FFI, l'effet transitif d'un wrapper non déclaré,
`i128`/`u128`/`isize`/`usize` à la frontière, `bool` à la frontière, un ABI
multi-plateforme, ni une fonction C qui modifie la mémoire ou termine le
processus.

## 3. Contradictions de spécification mesurées (à porter au responsable)

Ce sont des **défauts de niveau MUST**, pas des opinions d'agent. Aucune
correction n'est appliquée ici.

1. **`abi.md` ABI-2.5 et ABI-3.3 interdisent ce que la FFI implémente.**
   ABI-2.5 : « Pas de `unsafe` généré : le backend **MUST NOT** émettre de bloc
   `unsafe`, d'appel `unsafe`, ni de pointeur brut. » ABI-3.3 : « Le code généré
   ne contient **aucun** `extern`, `#[repr]`, ni `unsafe` : vérifiable par
   inspection de `--emit-rust` et par un test de conformité. » Or
   `emit_extern_block` émet `extern "C"` et un bloc `unsafe` par fonction
   `extern` — et l'audit `backend_audit` **l'exige**. Les deux MUST sont violés
   par une fonctionnalité par ailleurs spécifiée (G-3.10, `unsupported.md` §3).
2. **`abi.md` §3 refuse au parse toute frontière native** (ABI-3.1, ABI-3.2)
   alors que G-3.10 et `unsupported.md` §3.1 **acceptent** `extern "C" { … }`
   avec des primitives. Les deux textes décrivent le même objet avec des
   verdicts opposés.
3. **ABI-2.8** exige que `isize`/`usize` « suivent la cible » et **ABI-2.9**
   qu'aucune garantie de layout n'existe pour les types composés — cohérent avec
   la surface actuelle (aucun type composé n'est admis), mais la phrase
   « i128/u128 » n'a aucun contrat de largeur côté C (pas de `__int128` garanti
   MSVC) et aucun test ne l'exerce.
4. **ABI-1.2/1.3/1.4** (« aucun artefact destiné à être lié par un autre
   langage », « aucun ABI stable ») coexistent avec une FFI qui repose sur l'ABI
   **C** pour nommer et appeler des symboles. La spec doit dire lequel des deux
   régimes s'applique au sous-ensemble FFI, au lieu de laisser un lecteur
   conclure que la frontière C n'existe pas.

## 4. Contrats manquants (par ordre d'impact)

| # | Contrat absent | Pourquoi ce n'est pas un détail | État |
|---|---|---|---|
| 1 | **Aucun type pointeur / opaque**. Impossible de passer un tampon, un `*const c_char`, un handle, une struct C. | Ce n'est pas « FFI limitée », c'est « FFI scalaire uniquement ». Ajouter des pointeurs touche G-5.7, OWN (aliasing), l'ABI et la TCB : c'est une décision de langage, pas un wrapper. | `UNSUPPORTED` explicite (`unsupported.md` §3, G-5.7) |
| 2 | **Aucune édition de liens explicite** (`#[link]`, `-l`, chemin de bibliothèque refusés). | Seuls les symboles de la CRT/link par défaut sont appelables ; une bibliothèque native arbitraire n'est pas exprimable. Le mot « FFI » du dossier D7 surévalue la surface réelle. | `UNSUPPORTED` (attributs) ; non documenté comme limite de FFI |
| 3 | **Aucun contrat de propriété / libération**. | Il n'y a rien à libérer **parce que** rien de possédé ne traverse (pas de pointeur). Dès que (1) existera, ce contrat deviendra obligatoire : qui libère, quand, en cas d'échec ? | Sans objet aujourd'hui, **non écrit** pour demain |
| 4 | **Aucun contrat de durée de vie / emprunt**. | `&str` n'est pas FFI-safe ; aucune vue empruntée ne traverse. Une future vue `*const u8` + longueur exigera une règle explicite (OWN-3.3 interdit déjà le stockage). | Sans objet aujourd'hui, **non écrit** |
| 5 | **Aucun contrat d'unwinding / terminaison**. | Rust interdit l'unwind à travers `extern "C"` ; une fonction C qui `longjmp`, `abort()`, lève un signal ou termine le processus n'a **aucun** comportement spécifié vis-à-vis du hook de panic Ravum (`panic: …`, exit 1) ni du runner de tests. | **Absent** — ni spec, ni test |
| 6 | **Callbacks impossibles**. | Pas de type fonction, pas de pointeur de fonction : le C ne peut pas rappeler du Ravum. Ce n'est pas un manque d'implémentation, c'est inexprimable. À dire, sinon un lecteur suppose l'inverse. | Inexprimable (G-5.7, G-5.8) |
| 7 | **Effets transitifs non testés**. | Un wrapper non déclaré qui appelle `extern` doit propager `unsafe`+`ffi` jusqu'à l'entrée (EFF-2.9 MUST) ; un seul test couvre la déclaration, aucun ne couvre la propagation. | Implémenté, **non mesuré** |
| 8 | **`rt` + FFI non testé**. | EFF-4.7 (MUST) interdit `unsafe ffi` sous `rt` ; seul `pure` a un test. | Spécifié, **NOT_RUN** |
| 9 | **`i128`/`u128`/`isize`/`usize`/`bool` non exercés**. | Aucun cas de corpus n'utilise ces types à la frontière ; la conversion de largeur C n'est vérifiée nulle part. | **NOT_RUN** |
| 10 | **Sûreté du processus global non déclarée**. | La TCB inclut la bibliothèque C appelée ; aucun document ne dit que le binaire peut être corrompu par elle. | **Absent** (et c'est la phrase la plus importante de D7) |

## 5. Tests négatifs proposés → écrits en passe 11 (2026-11-13)

Chacun est un cas de conformance `compile-fail`/`run-fail` ou un test sema, avec
le code exact attendu. Aucun ne demande de changer le comportement. **État au
2026-11-13** : huit cas sont écrits et verts (`effects/ffi-rt-profile`,
`effects/ffi-transitive-wrapper`, `types/ffi-char-rejected`,
`types/ffi-reference-rejected`, `types/ffi-bool-boundary`,
`run/ffi-width-boundary`, `types/ffi-extern-in-impl`,
`types/unsafe-does-not-unlock-pointers`) ; le tableau indique l'écart quand le
comportement observé diffère de la proposition.

| Test proposé | Mode | Attendu | Ce qu'il verrouille | État 2026-11-13 |
|---|---|---|---|---|
| `types/ffi-rt-profile` | `compile-fail` (`--profile rt`) | `E-EFF-PROFILE` | EFF-4.7 | **écrit** sous `effects/ffi-rt-profile` |
| `effects/ffi-transitive-wrapper` | `compile-fail` (`--profile pure`) | `E-EFF-PROFILE` | propagation à travers 2 niveaux (EFF-2.9) | **écrit**, chaîne `main -> outer -> inner` vérifiée |
| `types/ffi-char-rejected` | `compile-fail` | `E-FFI-TYPE` | `char` non portable | **écrit** |
| `types/ffi-reference-rejected` | `compile-fail` | `E-FFI-TYPE` | `&i32` refusé | **écrit** (le cas `&str` dédié reste à ajouter) |
| `types/ffi-bool-boundary` | `compile-pass` | build OK | `bool` admis et wrapper compilé | **écrit** en `compile-pass` : aucun symbole CRT `_Bool` ne permet une valeur traversante, le cas ne prouve donc pas l'ABI runtime |
| `run/ffi-width-boundary` | `run-pass` | valeur attendue | un `i64` traverse réellement | **écrit** (`llabs(-9000000000) = 9000000000`) ; `u64`/`isize`/`usize` restent non exercés faute de symbole portable |
| `types/ffi-extern-in-impl` | `compile-fail` | `E-PARSE-EXPECTED` | G-3.11 (module uniquement) | **écrit**, mais le code **observé** est `E-UNSUPPORTED-EXTERN` : le cas enregistre le comportement réel, il n'amende pas la spec |
| `types/unsafe-does-not-unlock-pointers` | `compile-fail` | `E-UNSUPPORTED-UNSAFE` | un bloc `unsafe` ne crée aucune capacité | **écrit**, `unsupported: true` |
| `cli/doctor` (existant) | — | `ffi` visible dans l'inventaire | à étendre : `doctor` doit nommer la limite « pas de `#[link]`, pas de pointeurs » | **non fait** (ce n'est pas un cas de conformance) |

S'y ajoute un **test de contrat de lowering** (voir RFC-D2) : tout programme
contenant un `extern` doit produire un binaire dont le wrapper est le seul
`unsafe` ; le test `backend_audit` le fait déjà sur un programme construit à la
main — il manque la version pilotée par le corpus.

## 6. Ce que D7 peut et ne peut pas affirmer

**Peut être affirmé, avec preuve :** la FFI de Core 1.0 est une déclaration
`extern "C"` scalaire, appelable uniquement dans `unsafe`, à effets `unsafe`+
`ffi` inférés, refusée hors `unsafe` et sous `pure`, abaissée vers Rust en un
wrapper `unsafe` unique dont l'appel réel a été exécuté (`abs(-7) = 7`).

**Ne peut pas être affirmé :** que la FFI est « sûre », « auditée », « prête pour
une bibliothèque native », « compatible ABI stable », ou qu'elle permet
d'appeler autre chose que des symboles scalaires déjà liés par défaut. Le
wrapper Rust ne rend pas la bibliothèque C sûre : il déplace l'`unsafe` d'un
endroit visible (le source Ravum) vers un endroit généré et vérifiable **en
forme**, sans valider le contrat C.

**Recommandation (non contraignante) :** maintenir D7 en `OPEN`, garder la FFI
`experimental`, et conditionner toute ratification à : (a) la correction des
contradictions `abi.md` §2/§3 (points 1–2 ci-dessus), (b) les tests négatifs
§5, (c) une phrase explicite sur la TCB incluant la bibliothèque C. Une
ratification qui omettrait (c) donnerait un `PASS` de forme sur une frontière
dont la sûreté n'est pas démontrée.

## 7. Ce que cette RFC ne fait pas

- Elle ne ratifie pas D7, ne modifie aucun ADR, `abi.md`, `effects.md` ni le
  code ; les propositions §5 sont des cas à écrire, pas des cas écrits.
- Elle ne prétend pas que la FFI est inutile : elle mesure ce qu'elle permet
  aujourd'hui (scalaire → scalaire) et ce qu'elle interdit (tout le reste).
- Elle ne présente aucun test « absent » comme un succès : chaque ligne
  `NOT_RUN`/`NOT_TESTED` ci-dessus est une absence, pas un `PASS`.
