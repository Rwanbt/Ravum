# Dossier pour l'audit externe des sites `unsafe` (D4/R13)

| Champ | Valeur |
|---|---|
| Objet | Préparer un audit **externe et indépendant** des sites `unsafe` du langage Ravum Core 1.0 (crates `ravum-lang-*`) |
| Statut | **Dossier préparé par l'agent, audit NON réalisé.** Une revue de l'agent n'est pas un audit. À commander par le responsable (décision D4/R13). |
| Source | branche `claude/trusting-cerf-4hhv5a`, à partir de `0345cd7` (2026-11-20) |
| Toolchain | rustc 1.95.0, Cranelift 0.116, Linux x86_64 (mesures) ; Ubuntu 26.04 + PipeWire pour l'essai `cpal` |
| Inventaire faisant foi | [`docs/ffi/UNSAFE-INVENTORY.md`](../ffi/UNSAFE-INVENTORY.md) (contrôlé par `tools/unsafe_audit.py` en CI) |

## 1. Périmètre demandé à l'auditeur

| Priorité | Site | Emplacement | Nature |
|---|---|---|---|
| **P1** | #7, #7b, #7c | `crates/ravum-lang-jit/src/lib.rs` `mod exec::{call, call1, call2}` (~l. 1160–1190) | `transmute` d'une adresse de code Cranelift en `extern "C" fn` puis appel |
| **P1** | #8 | `crates/ravum-lang-jit/src/hot.rs` `mod raw::{take, reclaim}` (~l. 990–1007) | `Box::from_raw` sur des pointeurs passés par `AtomicPtr` entre thread éditeur et thread temps réel |
| **P1** | #9 | idem `raw::free_module` (~l. 1010) | `JITModule::free_memory` : libération de code machine pendant que le thread temps réel tourne |
| P2 | #7d | `lib.rs` `exec::{rv_print, rv_heap, OutputSink::text}` (~l. 1210–1310) | fonctions hôtes appelées par le code compilé : `&mut *ctx`, `&mut *(ctx.out as *mut OutputSink)`, `*texts.add(i)` |
| P3 | #1–#6 | `crates/ravum-lang-cli/src/commands/watch.rs` `mod signal` | `SetConsoleCtrlHandler` / `signal(2)` (gestion de Ctrl-C) |
| hors code | code **généré** par Cranelift | — | repose sur la validation de l'IR, le vérificateur Cranelift et l'oracle différentiel (voir §4) |
| hors code | code Rust **généré** pour un `extern "C"` / bloc DSP | `ravum-lang-codegen` | un seul `#[allow(unsafe_code)]` par programme (`SAFETY.md` §2, §8) |

Tous les autres crates du langage portent `#![forbid(unsafe_code)]`
(`syntax`, `sema`, `codegen`, `lsp`, `ir`, `incr`, `audio`) ou
`#![deny(unsafe_code)]` avec les seules exceptions listées (`cli`, `jit`).
Les tests (`tests/*.rs`) contiennent quelques `unsafe` de banc d'essai
(allocateurs compteurs, `clock_gettime`) : hors périmètre produit.

## 2. Invariants à vérifier (hypothèses de sûreté)

**#7/#7b/#7c — appel de code JIT.**
1. `code` provient de `JITModule::get_finalized_function` d'un module
   finalisé et vivant pendant l'appel (`&Compiled`, ou `Version` tenue par
   le `RtRunner`, cf. #9).
2. La signature transmutée est exactement celle déclarée à Cranelift :
   `(*mut Ctx) -> R`, `(A, *mut Ctx) -> R`, `(A, B, *mut Ctx) -> R` ; `R`/`A`/`B`
   choisis d'après l'IR (`EntryKind::of`, `cl_ty`). `update` refuse une
   version dont la signature d'entrée diffère (`Refusal::EntrySignature`).
3. `Ctx` est `#[repr(C)]` ; le code compilé n'accède qu'aux décalages
   0, 4, 8, 16, 24, 32, 40, 48 ; `arm()` positionne `stack_floor` avant chaque
   appel (garde de pile).
4. Le tampon d'état DSP (`call2`) : `Box<[u64]>` aligné sur 8, taille ≥
   `StateInfo::size`, emprunté exclusivement pendant l'appel ; accès à
   décalages de champs fixes ou par index vérifié.

**#8 — transfert de `Box<Version>`.**
5. Tout pointeur non nul d'un emplacement (`pending`, `retired`) vient de
   `Box::into_raw` et en sort par `swap` (un seul côté le reprend).
6. Le runner ne place une version dans `retired` que si l'emplacement est
   vide (garde de `poll_event`) : au plus une version en transit.

**#9 — libération de code machine.**
7. `collect` (éditeur) ne libère un module que si aucune version de
   génération ≥ `min(active_generation, previous_generation)` ne le
   référence.
8. Le runner publie `previous_generation` **avant** `active_generation` à
   l'adoption ; au retour arrière il échange deux générations déjà tenues.
   Hypothèse : toute lecture concurrente de la paire par `collect` donne une
   borne ≤ à la plus ancienne génération réellement tenue. **Point à
   vérifier en priorité** (ordonnancement mémoire `SeqCst`, entrelacements
   adoption / retour arrière / publication non adoptée / migration HR-1.7).
9. Le runner n'exécute du code que dans `process_*`, sur son thread, via
   `current`/`previous` ; une version retirée n'est plus exécutée.

**#7d — fonctions hôtes.** 10. `ctx.out` vaut 0 ou l'adresse d'un
`OutputSink` vivant sur la pile de l'appelant pendant tout l'appel ; un
module contenant `Print`/tas est refusé pour une entrée temps réel (INV-05).

## 3. Preuves existantes (toutes `TESTED`, aucune `PROVED`)

| Preuve | Commande | Ce qu'elle montre |
|---|---|---|
| inventaire = code | `python3 tools/unsafe_audit.py` | forme de l'inventaire (attributs, `allow` locaux) |
| oracle différentiel | `cargo test -p ravum-lang-jit --test differential` | code JIT = interpréteur de référence = machine indépendante sur un corpus |
| hot swap sous thread temps réel | `cargo test -p ravum-lang-jit --test hot_swap` | 20 éditions, 0 allocation/libération sur le thread RT ; `concurrent_publish_and_collect_never_runs_freed_code` : 300 publications sans attente + `collect` à chaque fois, runner en boucle serrée, 1 version sur 7 qui trappe : 0 valeur invalide, aucun crash |
| budget temps réel, garde de pile | `cargo test -p ravum-lang-jit --test rt_budget` | appel borné (`RT_FUEL`), garde de pile sur threads de 512 KiB / 2 MiB |
| état DSP, migrate | `--test dsp_hot`, `--test migrate` | écritures partielles annulées sur trap, migration bit à bit |
| replay | `--test replay` | session rejouée bit à bit |
| essai réel `cpal` | `docs/reports/V1-AUDIO-HOST.md` | lecture sur périphérique, 0 allocation Rust dans `process` |

## 4. Limites connues des preuves

- Un test ne prouve pas l'absence de course ; aucun modèle formel des
  entrelacements (#9). Outils non utilisés : **Miri** (n'exécute pas de code
  JIT), **loom** (pas de modèle de la boîte aux lettres), **TSan/ASan**
  (toolchain nightly requise).
- Le code généré par Cranelift est fait confiance via son vérificateur
  (actif en debug) ; pas de vérification indépendante en release.
- Sites #1–#6 testés seulement par `watch_continuity.rs` (Unix).

## 5. Questions ouvertes pour l'auditeur

1. L'argument de #9 tient-il pour **tous** les entrelacements, y compris
   `collect` pendant `collect_migration` et une publication refusée ?
   Faut-il une époque/hazard pointer plutôt que la paire de générations ?
2. `SeqCst` partout : suffisant et nécessaire ? Une relaxation est-elle sûre ?
3. Le garde de pile (`get_stack_pointer` comparé à `stack_floor`) couvre-t-il
   les trames des appels hôtes (`rv_heap`) et les `stack_slot` Cranelift ?
4. Les `transmute` de pointeurs de fonction respectent-ils l'ABI
   `extern "C"` de Cranelift (`CallConv::triple_default`) sur toutes les
   cibles visées (x86_64, aarch64, Windows) ?
5. Recommandations pour outiller : modèle `loom` de la boîte aux lettres,
   exécution sous TSan en CI nightly.

## 6. Livrable attendu de l'audit

Pour chaque site : verdict (sûr / sûr sous hypothèses / non sûr), hypothèses
retenues, défauts trouvés avec scénario, recommandations. Le registre
`CORE-1.0-GATE-MATRIX.md` ne mentionnera un audit qu'après réception de ce
livrable signé.
