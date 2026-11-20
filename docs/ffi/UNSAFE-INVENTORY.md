# Inventaire `unsafe` du langage Ravum Core 1.0 (D7, outillé le 2026-11-14)

> **Statut : inventaire factuel, `PENDING_HUMAN_APPROVAL` pour D7.** Ce document
> ne ratifie rien : il enregistre **tous** les sites `unsafe` des crates du
> langage et la façon dont le compilateur les borne. La ratification D7 et
> l'audit externe (D4/R13) restent des décisions humaines.
> Contrat de frontière : [`SAFETY.md`](SAFETY.md). Mesure de l'écart :
> [`PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) R5/D7.

## 1. Pourquoi un inventaire, et ce qu'il change

`SAFETY.md` §4 (programme D7, étage 1) demande que `unsafe` n'existe que dans des
modules audités et listés, avec une comparaison en CI. Jusqu'au 2026-11-14 cette
règle était une **intention** : rien, dans le compilateur, n'empêchait d'ajouter
un `unsafe` dans `sema` ou `codegen`.

Deux verrous sont désormais en place, et ils sont de nature différente :

1. **Le compilateur Rust** (`#![forbid(unsafe_code)]` / `#![deny(unsafe_code)]`)
   rend l'ajout d'un `unsafe` non listé **impossible à compiler** dans les
   crates concernés. C'est la seule garantie mécanique, et elle porte sur le
   code présent dans ces crates.
2. **`tools/unsafe_audit.py`** vérifie la *forme* de l'inventaire que le lint ne
   peut pas vérifier : les cinq racines portent l'attribut attendu, aucun
   `#![allow(unsafe_code)]` global n'existe, et les seuls
   `#[allow(unsafe_code)]` locaux sont les deux variantes du module de signaux
   du CLI. Le script tourne dans `gate-contracts.yml` et a ses propres tests.

## 2. Les sites, exhaustivement

| # | Site | Code | Pourquoi `unsafe` | Argument de sûreté |
|---|---|---|---|---|
| 1 | `crates/ravum-lang-cli/src/commands/watch.rs`, `mod signal` (Windows) | `unsafe extern "system" { fn SetConsoleCtrlHandler(...) }` | API Win32 sans crate externe | signature reproduite à l'identique (`HandlerRoutine = unsafe extern "system" fn(u32) -> i32`) ; installation best-effort |
| 2 | idem | `unsafe extern "system" fn handler(_control_type: u32) -> i32` | callback exigé par Win32 | ne fait que `STOP.store(true, SeqCst)` puis retourne `1` ; aucune allocation, aucun verrou, aucune panique |
| 3 | idem | `unsafe { SetConsoleCtrlHandler(Some(handler), 1) }` | appel de l'API | pointeur vers une fonction `extern "system"` de durée de vie statique ; valeur de retour ignorée (best-effort) |
| 4 | `crates/ravum-lang-cli/src/commands/watch.rs`, `mod signal` (Unix) | `unsafe extern "C" { fn signal(sig: i32, handler: usize) -> usize; }` | `signal(2)` sans crate externe | `SIGINT` (2) et `SIGTERM` (15) sont installées (SIGTERM : décision V2-K1, utilisé aussi par `run`) ; pointeur passé en `usize` selon l'ABI de la plateforme |
| 5 | idem | `unsafe extern "C" fn handler(_sig: i32)` | callback exigé par `signal(2)` | ne fait que `STOP.store(true, SeqCst)` ; travail async-signal-safe uniquement |
| 6 | idem | `unsafe { signal(2, handler as usize) }` | appel de l'API | pointeur de fonction valide ; résultat ignoré (best-effort) |

Aucun autre site `unsafe` n'existe dans `ravum-lang-syntax`, `ravum-lang-sema`,
`ravum-lang-codegen`, `ravum-lang-lsp` ni dans le reste de `ravum-lang-cli` : ces
quatre crates portent `#![forbid(unsafe_code)]`, le CLI porte
`#![deny(unsafe_code)]` et les deux `#[allow(unsafe_code)]` ci-dessus.

### Ajout du 2026-11-19 — backend JIT (T15, V1) — **audit humain requis**

| # | Site | Code | Pourquoi `unsafe` | Argument de sûreté |
|---|---|---|---|---|
| 7 | `crates/ravum-lang-jit/src/lib.rs`, `mod exec` (seul `#![allow(unsafe_code)]` du crate, qui porte `#![deny(unsafe_code)]`) | `unsafe { std::mem::transmute::<*const u8, extern "C" fn(*mut Ctx) -> R>(code) }` puis appel | exécuter le code machine produit par Cranelift | `code` vient de `JITModule::get_finalized_function` pour une fonction déclarée avec la signature `(*mut Ctx) -> R` ; `R` est choisi d'après le type de retour déclaré ; le module est finalisé et vit pendant l'appel (`&Compiled`) ; `Ctx` est `#[repr(C)]` (décalages 0 à 56 : code, profondeur, carburant, table, sortie, scratch, `stack_floor`, `stack_budget`) ; le code compilé lit/écrit 0..48, `stack_budget` n'est lu que par `exec::arm` ; `arm` fixe `stack_floor` avant chaque appel (garde de pile, 2026-11-20) |

`ravum-lang-ir` et `ravum-lang-incr` portent `#![forbid(unsafe_code)]`. Le code
**généré** par Cranelift n'est pas couvert par cet argument : sa sûreté repose
sur la validation de l'IR (`Module::validate`), le vérificateur Cranelift (actif
en debug) et l'oracle différentiel (`crates/ravum-lang-jit/tests/differential.rs`).
Statut : `TESTED`, non audité (D4).

### Ajout du 2026-11-19 — hot swap natif (T16, V1) — **audit humain requis**

| # | Site | Code | Pourquoi `unsafe` | Argument de sûreté |
|---|---|---|---|---|
| 7b | `crates/ravum-lang-jit/src/lib.rs`, `mod exec::call1` | `transmute::<*const u8, extern "C" fn(A, *mut Ctx) -> R>` | appeler l'entrée `fn(f32)->f32` / `fn(f64)->f64` | même argument que #7 ; `A`/`R` fixés par `EntryKind`, vérifié sur la signature IR avant toute publication |
| 8 | `crates/ravum-lang-jit/src/hot.rs`, `mod raw::{take, reclaim}` | `Box::from_raw` | rendre une `Box<Version>` passée par la boîte aux lettres atomique | chaque pointeur non nul vient de `Box::into_raw` et sort de son emplacement par un `swap` atomique : un seul côté le reprend |
| 9 | idem, `raw::free_module` | `JITModule::free_memory` | libérer le code machine d'anciennes versions | appelé seulement pour un module qu'aucune version vivante ne référence ; une version cesse d'être vivante quand le thread temps réel publie (`active_generation`/`previous_generation`) qu'il en tient de plus récentes ; il n'exécute du code que dans `process_*`, via la version qu'il tient |
| 7c | `crates/ravum-lang-jit/src/lib.rs`, `mod exec::call2` (IR v1.1, DSP) | `transmute::<*const u8, extern "C" fn(A, B, *mut Ctx) -> R>` | appeler `process_sample(&mut State, f32)` | même argument que #7 ; l'état est un tampon `Box<[u64]>` possédé par le `RtRunner`, aligné sur 8, de taille ≥ `StateInfo::size`, emprunté exclusivement pendant l'appel ; le code n'y accède qu'à des décalages de champs fixes ou via un index vérifié ; **aussi** `migrate(out, old)` (HR-1.7, côté éditeur) : `out` et `old` sont deux tampons `Box<[u64]>` alloués par l'éditeur, de la taille de `State` et de `T`, empruntés exclusivement pendant l'appel |
| 7d | `crates/ravum-lang-jit/src/lib.rs`, `mod exec::rv_print` (IR v1.2, `io::print*`) et `exec::rv_heap` (IR v1.3, `String`/`Vec`, même argument ; le tas vit dans l'`OutputSink` de l'exécution) | `&mut *ctx`, `&mut *(ctx.out as *mut OutputSink)`, `*texts.add(i)` | fonction hôte appelée par le code compilé pour `Inst::Print` | `ctx` est le `&mut Ctx` exclusif de l'appel en cours (cf. #7) ; `ctx.out` vaut 0 (rien n'est écrit) ou l'adresse d'un `OutputSink` sur la pile de l'appelant (`run_with_output`, `run_nullary_with_output`), vivant pendant tout l'appel et non touché ailleurs ; `texts`/`n` empruntent la table des messages pour la même durée et l'index est comparé à `n` ; jamais exécutée sur le thread temps réel (un module contenant `Print` est refusé pour une entrée temps réel, INV-05) |

## 3. Ce que l'inventaire ne couvre pas

### ABI DSP 3 (L5b)

| Site | Opérations et garanties exigées |
|---|---|
| `crates/ravum-patch-host/src/opaque_block.rs` | Chargement de symboles et appels C d’un artefact natif de confiance. Les portes ABI et manifeste précèdent la résolution du bloc. Les digests sont copiés tant que la bibliothèque vit. Les métadonnées seules ne rendent pas sûr un artefact hostile. |
| `crates/ravum-patch-host/src/opaque_block.rs`, `StateBuffer` | Allocation et libération selon le layout contrôlé. Chaque tampon a un propriétaire, et `Send` ne déplace que cet état scalaire sans emprunt. Les tampons restent vivants pendant les appels et sont libérés hors du traitement. |
| `crates/ravum-patch-host/src/opaque_block.rs`, `migrate_state` | Copie brute vers un tampon séparé de même disposition. Le digest source est comparé avant la migration. La nouvelle zone ne devient active qu’après confirmation de l’initialisation par le module. |
| `crates/ravum-patch-host/src/opaque_block/live.rs` | L’unique lecteur audio remplit la copie avant le signal `Release`. L’éditeur lit après `Acquire` et ne modifie jamais la copie. L’adoption échange le tampon avec celui du slot. L’éditeur récupère et libère l’ancien tampon après la fin des emprunts `ArcSlot`. |
| `crates/ravum-lang-codegen/src/emit/emitter_reload.rs`, module généré `rv_dsp_abi` | Références temporaires sur l’état `repr(C)` et sur des tranches non superposées de `frames` éléments. La migration consomme une copie alignée et écrit dans une nouvelle zone. La panique est interceptée dans le module qui l’a produite. |
| `crates/ravum-dsp-harness/src/evidence.rs`, `opaque_block_evidence` | Le caller garantit la provenance de l’artefact exécutable. Le chargement vérifie l’identité du binaire avant d’exercer le traitement. |

- **Le code généré.** `ravum-lang-codegen` produit un fichier Rust qui porte
  `#![forbid(unsafe_code)]` ; pour un programme avec `extern "C"`, l'en-tête
  passe à `#![deny(unsafe_code)]` et **un seul** `#[allow(unsafe_code)]`
  encadre le wrapper `rvm_ffi_wrap_<id>`. Ce n'est pas un site `unsafe` du
  compilateur, mais c'est un `unsafe` du **produit** : il est couvert par les
  tests backend (un seul `unsafe`, `link_name` correct) et par
  [`SAFETY.md`](SAFETY.md) §2. Un **bloc DSP** (`build --emit dsp-block`,
  RFC-E11-DSP-BLOCK DSP-1.3) porte de même `#![deny(unsafe_code)]` et un seul
  `#[allow(unsafe_code)]`, sur le module généré `rv_dsp_abi` : il convertit les
  pointeurs de bloc de l'hôte en tranches, recopie l'état hôte (`z1`, `z2`)
  dans `State` et retour ; voir [`SAFETY.md`](SAFETY.md) §8.
- **La bibliothèque C appelée.** Elle n'est pas compilée par Ravum ; aucun
  inventaire ne peut la borner. C'est la phrase TCB de `SAFETY.md` §1.
- **Les dépendances.** L'inventaire porte sur les crates du langage, pas sur
  `serde`/`serde_json` ; leur `unsafe` éventuel n'est pas audité ici.
- **L'audit externe.** Un inventaire outillé n'est pas une revue humaine
  indépendante : D4/R13 restent `BLOCKED`.

## 4. Vérification

```
python tools/unsafe_audit.py
  unsafe inventory (D7): ...
  unsafe_audit: PASS — the code matches the audited inventory

python -m unittest discover -s tools/tests -p "test_unsafe_audit.py" -v
  7 tests, 0 failure
```

Les deux commandes tournent aussi en CI : le script dans `gate-contracts.yml`,
les tests dans `tools/tests`.

## 5. Mise à jour de l'inventaire

Tout nouveau `unsafe` exige, dans la même modification : (a) un argument de
sûreté écrit sur le site (`// SAFETY:`), (b) une ligne dans le tableau §2,
(c) l'ajout du fichier au bon compte dans `ALLOWED_LOCAL_ALLOWS` de
`tools/unsafe_audit.py`, (d) un test qui échoue si le site disparaît. Élargir
`ALLOWED_LOCAL_ALLOWS` sans revue revient à affaiblir D7 : le script échoue
délibérément dans ce cas.

## Revue interne des sites #7b, #8, #9 (2026-11-20)

> Revue **interne** par l'agent de développement, **pas un audit externe**
> (D4/R13 restent des décisions humaines). Preuve : `TESTED`.

- **#7b `call1`** : le `transmute` vers `extern "C" fn(A, *mut Ctx) -> R`
  n'est atteint que par `process_f32`/`process_f64`, qui commencent par
  `assert_eq!(self.kind, …)` ; `EntryKind` est calculé une fois au
  démarrage depuis la signature IR et `update` refuse toute version dont la
  signature d'entrée diffère (`Refusal::EntrySignature`, testé). Le pointeur
  vient de la table de la version tenue par le runner (code vivant, cf. #9).
- **#8 `take`/`reclaim`** : chaque pointeur non nul d'un emplacement
  (`pending`, `retired`) provient de `Box::into_raw` et en sort par un
  `swap` atomique ; aucun autre chemin ne lit l'emplacement pour en faire
  une `Box`. Le runner ne retire une version (`retired`) qu'après que
  l'éditeur a vidé l'emplacement précédent (garde de `poll_event`) : au plus
  une version en transit, jamais deux propriétaires.
- **#9 `free_module`** : `collect` libère un module seulement si aucune
  version de génération ≥ `min(active, previous)` ne le référence. Le runner
  publie `previous_generation` **avant** `active_generation` (adoption) et,
  au retour arrière, échange deux générations déjà tenues : toute lecture
  concurrente de la paire par `collect` donne une borne ≤ à la plus vieille
  génération réellement tenue (analyse des entrelacements : adoption, retour
  arrière, publication non adoptée). Une version retirée n'est plus
  exécutée : le runner n'exécute du code que dans `process_*`, sur son
  thread, via `current`/`previous`.

Test adversarial ajouté :
`hot_swap.rs::concurrent_publish_and_collect_never_runs_freed_code` —
300 publications sans attente, `collect` après chacune, runner en boucle
serrée, 1 version sur 7 qui trappe (retour arrière effectif vérifié) :
**0 valeur hors des versions publiées, aucun crash** (debug : ~3·10⁵ appels
par exécution, 3 exécutions ; release : ~1,2·10⁶ appels), modules vivants
bornés (≤ 4 : courante, précédente, publication non adoptée, module de base).

Limites : un test ne prouve pas l'absence de course ; pas de Miri (le code
JIT n'est pas exécutable sous Miri) ni de sanitizer (toolchain stable).
