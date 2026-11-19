# RFC-E11-CONCURRENCY — Threads, canaux et `async` dans Core 1.0

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-E11-CONCURRENCY` |
| Statut | **`PROPOSED`** — périmètre fixé par [D-19-3](../decision/DECISIONS-2026-11-19.md) : phases 1 **et** 2 dans la 1.0 |
| Date | 2026-11-19 |
| Auteur | agent de développement |
| Origine | [D-18-2](../decision/DECISIONS-2026-11-18.md) (périmètre 1.0 élargi) ; ferme une partie de T6 |
| Remplace | options 3 et 4 de [`RFC-PERIMETRE-UNIVERSEL-D6`](RFC-PERIMETRE-UNIVERSEL-D6.md) pour la 1.0 |
| Portée | `spec/language/{effects,stdlib,ownership,unsupported}.md`, `crates/ravum-lang-sema` (résolution, types, effets, ownership), `crates/ravum-lang-codegen` (prélude), conformance |

> **Ce que cette RFC ne promet pas.** Ni absence d'interblocage, ni équité de
> l'ordonnanceur, ni temps réel. Elle promet **l'absence de course de données
> dans un programme accepté**, et rien de plus fort.

## 1. Problème

`unsupported.md` §5 refuse aujourd'hui threads, canaux, atomiques, `Mutex`,
`Arc`, `async` et l'effet `async`. D-18-2 met la concurrence dans la 1.0. Il
faut une surface qui :

1. reste compatible avec un langage **sans closures, sans traits, sans `Box`,
   `Rc` ni `Cell`** (`unsupported.md` §2, §3) ;
2. garde la preuve d'ownership **mono-thread** valide (aucune règle `OWN-*`
   existante ne change de sens) ;
3. est qualifiable : TSan est `NOT_APPLICABLE` aujourd'hui faute de chemin
   concurrent ; il doit devenir un job bloquant.

## 2. Observation structurante (vérifiable dans le dépôt)

Un type Ravum accepté ne peut contenir **ni pointeur partagé, ni mutabilité
intérieure** : `Rc`, `Cell`, `Box`, pointeurs bruts et `static` sont refusés.
Les seules données non possédées sont les références `&T` / `&mut T`. Donc :

> **Un type sans référence est transférable entre threads.** En termes Rust,
> tout type Ravum sans `&` est `Send + 'static`.

C'est la règle `CONC-2.1` proposée ; elle remplace un système de traits
`Send`/`Sync` que le langage n'a pas.

## 3. Surface proposée — phase 1 : threads et canaux

> **Phase 1 implémentée** (2026-11-19) : [`stdlib.md`](../../spec/language/stdlib.md)
> §18 (STD-18.1 à 18.9), [`concurrency.md`](../../spec/language/concurrency.md)
> révisé, FeatureId F-STD-037, 10 cas de conformance, test de charge
> `crates/ravum-lang-cli/tests/thread_stress.rs`. Écarts avec le texte
> ci-dessous, retenus à l'implémentation :
> - un canal se crée par `sync::channel()` qui rend un `Channel<T>` ; on en tire
>   `sender(&c)` et `receiver(c)` (§3.1, sans `let (a, b)` ni turbofish) ;
> - une panique dans un thread termine le processus (`panic:`, code 1) au lieu
>   d'être relancée par `join` : c'est le comportement de `main`, et il est
>   déterministe ;
> - `send` rend `bool` (`false` si le récepteur est détruit) ;
> - **TSan bloquant** (§5) : `.github/workflows/tsan.yml` et
>   `tools/tsan_run.py` (2026-11-19), sur tous les cas `run-pass` concurrents
>   (threads, canaux, TCP, HTTP), 10 exécutions chacun, avec un témoin de course
>   qui doit être signalé ; la bibliothèque standard n'est pas instrumentée.

Module `std::thread` et `std::sync`, effet **`thread`** (nouveau groupe) :

```rust
use std::thread;
use std::sync;

fn worker(input: Vec<i64>) -> i64 effects() { ... }

fn main() -> () effects(io, thread) {
    let h: thread::Handle<i64> = thread::spawn(worker, vec_input);
    let total: i64 = thread::join(h);          // panique propagée, voir CONC-3.4
    let (tx, rx) = sync::channel::<String>();  // forme exacte : §3.1
    sync::send(tx, "x".to_string());
    let m: Option<String> = sync::recv(rx);    // None quand tous les émetteurs sont détruits
}
```

| ID | Règle proposée | Niveau |
|---|---|---|
| CONC-2.1 | Un type est *transférable* s'il ne contient, transitivement, aucune référence. | MUST |
| CONC-3.1 | `thread::spawn(f, arg)` exige une **fonction nommée** `f: fn(A) -> R` (pas de closure), `A` et `R` transférables ; sinon `E-CONC-NOT-TRANSFERABLE`. | MUST |
| CONC-3.2 | `arg` est **déplacé** dans le thread ; toute utilisation ultérieure est `E-OWN-USE-AFTER-MOVE` (règle existante, aucune nouvelle). | MUST |
| CONC-3.3 | `f` ne doit pas déclarer d'effet absent de l'appelant ; `spawn` ajoute l'effet `thread` à l'appelant. | MUST |
| CONC-3.4 | `thread::join(h)` rend `R` ; si le thread a paniqué, `join` panique avec le même message (pas de `Result` silencieux). | MUST |
| CONC-3.5 | Un `Handle` non joint est joint à la fin de `main` ; le processus ne se termine pas avec un thread vivant. | MUST |
| CONC-4.1 | `sync::channel::<T>()` exige `T` transférable ; `Sender<T>` est clonable (`sync::clone_sender`), `Receiver<T>` ne l'est pas. | MUST |
| CONC-4.2 | `sync::recv` bloque ; il rend `None` quand tous les `Sender` sont détruits. Pas de `recv_timeout` en 1.0 (BL-4 : aucune horloge). | MUST |
| CONC-5.1 | Aucun état mutable partagé : `Mutex`, `RwLock`, `Arc`, atomiques restent `E-RES-UNKNOWN`. | MUST |

**Abaissement.** Prélude généré : `std::thread::spawn(move || f(arg))`,
`std::sync::mpsc`. Le Rust émis reste vérifié par `rustc` ; si `rustc` refuse
un `spawn` que le checker a accepté, c'est un bug du checker (UNS-0.3), jamais
un repli.

### 3.1 Questions de syntaxe ouvertes

`let (tx, rx) = ...` est refusé (`unsupported.md` §3, pattern de `let`), et le
turbofish aussi. Proposition : `sync::channel()` rend une struct
`sync::Channel<T>` avec `sync::sender(c)` et `sync::receiver(c)`, le type venant
de l'annotation `let c: sync::Channel<String>`. Aucune exception de grammaire.

## 4. Surface proposée — phase 2 : `async`

Proposition **minimale et déterministe** : `async fn` + `.await`, un exécuteur
**mono-thread à ordre fixé** (FIFO des tâches prêtes), seulement des futures
d'E/S réseau (RFC-E11-NET). Aucun `async` sans `net` : seul, il n'apporte rien
(D6 §3, option 3).

| ID | Règle proposée | Niveau |
|---|---|---|
| CONC-6.1 | `async fn` rend un futur opaque ; `.await` hors `async fn` est `E-CONC-AWAIT-OUTSIDE`. | MUST |
| CONC-6.2 | L'ordre d'exécution des tâches prêtes est FIFO et reproductible pour une même suite d'événements d'E/S. | MUST |
| CONC-6.3 | Une référence ne traverse pas un `.await` (`E-CONC-REF-ACROSS-AWAIT`) : pas d'emprunt suspendu. | MUST |

**D-19-3** : les deux phases sont dans la 1.0, livrées **séparément** (phase 1 d'abord). La phase 2
est la plus coûteuse (nouveau modèle d'exécution, états d'emprunt à travers les
suspensions) ; un serveur HTTP 1.0 peut s'écrire en phase 1 avec un thread par
connexion.

## 5. Qualification (critères de falsification)

- TSan réactivé sur Linux, **bloquant**, sur tout le corpus `run-pass` qui
  utilise `thread` ; un rapport TSan = échec.
- Stress : 1 000 exécutions d'un programme producteur/consommateur, sortie
  identique attendue (canal FIFO par émetteur) ; une divergence = échec.
- Cas `compile-fail` pour chaque code `E-CONC-*`, et un cas par refus maintenu
  (`Mutex`, `Arc`, atomiques).
- Différentiel avec la machine de référence : `ravum-core-machine` reste
  limitée au cœur pur ; elle **refuse** `thread` (fault), elle ne l'évalue pas.

## 6. Coût mesuré du point de départ

Effets : `facts.rs` connaît déjà `Effect::Async` et `Effect::Net` (refusés,
test `only_async_and_net_are_unsupported`) ; `thread` est un groupe nouveau.
Aucune ligne de code n'est écrite par cette RFC.

## 7. Décisions attendues du responsable

1. Accepter CONC-2.1 (transférable = sans référence) comme substitut de `Send`.
2. Nom du groupe d'effet : `thread` (proposé) ou `async`/`concurrency`.
3. ~~Phase 2 dans la 1.0~~ — tranché par D-19-3 : oui.
4. TSan bloquant comme condition de `PASS` de T6.
