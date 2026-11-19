# Ravum Core 1.0 — Concurrence (modèle candidat)

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.6, §5.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR** (notamment pas ADR-008 « concurrence, synchronisation et annulation » ni ADR-024 « thread audio »).
>
> **Révision du 2026-11-19.** Les décisions [D-18-2](../../docs/decision/DECISIONS-2026-11-18.md) et [D-19-3](../../docs/decision/DECISIONS-2026-11-19.md) mettent la concurrence dans la 1.0 ([`RFC-E11-CONCURRENCY`](../../docs/rfc/RFC-E11-CONCURRENCY.md)). La phase 1 (threads et canaux, [`stdlib.md`](stdlib.md) §18) est implémentée ; la phase 2 (`async`) ne l'est pas encore.

## 1. Décision : threads et canaux, sans état partagé

| ID | Règle | Niveau |
|---|---|---|
| CONC-1.1 | Un programme qui n'appelle pas `thread::spawn` s'exécute sur **un seul** thread, séquentiellement. Avec `spawn`, il exécute plusieurs threads ([`stdlib.md`](stdlib.md) §18). | MUST |
| CONC-1.2 | Les **seules** primitives de concurrence sont `thread::spawn`, `thread::join` et les canaux de `std::sync`. Il n'existe ni `Mutex`, ni `RwLock`, ni `Arc`, ni `atomic`, ni `select`, ni `sleep`, ni domaine parallèle ; `async`/`await` attend la phase 2. | MUST |
| CONC-1.3 | Le groupe d'effets `thread` marque toute fonction qui crée un thread ou utilise un canal. `async` marque toute `async fn` et tout appel de `std::task` (§6, D-19-9) ; `rt` est réservé, sans sémantique ([`effects.md`](effects.md)). | MUST |
| CONC-1.4 | La seule « concurrence » possible est **externe** au langage : plusieurs processus `ravum-lang run` lancés par l'utilisateur. Ils ne partagent aucun état mémoire ; les interactions passent par le système de fichiers ou les E/S standard, hors du modèle Ravum. | MUST |
| CONC-1.5 | Aucun programme Ravum ne peut créer, lire ou écrire un état **mutable partagé** entre deux threads : une valeur est déplacée dans un thread (STD-18.3) ou envoyée par un canal, jamais partagée. | MUST |

```rv
// POSITIF — un thread de calcul, résultat rendu par join
fn carre(n: i64) -> i64 effects() { return n * n; }
fn main() -> () effects(io, thread, alloc) {
    let h: JoinHandle<i64> = thread::spawn(carre, 12);
    io::println(thread::join(h).to_string().as_str());
}
```

```rv
// NÉGATIF — E-RES-UNKNOWN (aucun état partagé)
fn main() { let m = Mutex::new(0); }
// NÉGATIF — E-UNSUPPORTED-ASYNC (bloc async ; `async fn` est accepté, §6)
fn f() { let x = async { 1 }; }
// NÉGATIF — E-EFF-PROFILE (async sous le profil pure)
async fn g() -> i64 effects(async) { 1 }
```

## 2. Ce que le langage ne promet pas

| ID | Non-promesse | Niveau |
|---|---|---|
| CONC-2.1 | L'absence de course de données découle de deux règles vérifiées : aucun état mutable partagé (CONC-1.5) et aucune référence ne traverse un thread (STD-18.1). C'est une propriété **testée** (corpus, TSan), jamais `PROVED` ; aucun modèle mémoire n'est revendiqué. | MUST |
| CONC-2.2 | Il n'existe **aucun** trait `Send`/`Sync` ni borne de concurrence : la transférabilité est **structurelle** (un type sans référence, STD-18.1). Les génériques n'ont pas de bornes ([`types.md`](types.md) TYP-11.6). | MUST |
| CONC-2.3 | Il n'existe **aucun** ordre mémoire exposé, aucune barrière, aucune atomique. Le seul ordre garanti est celui des canaux : une valeur envoyée est reçue après son envoi, et les valeurs d'un même `Sender` arrivent dans l'ordre (STD-18.7). | MUST |
| CONC-2.4 | Aucune garantie de « fairness », de « pas de deadlock », ni de « pas de famine » : un `recv` sur un canal dont un `Sender` reste vivant peut attendre indéfiniment. | MUST |
| CONC-2.5 | Aucune garantie de latence, de gigue, ni de temps réel. `rt` est **expérimental** et ne certifie **pas** le hard-RT (design §3.6, [`effects.md`](effects.md) EFF-4.6). | MUST |
| CONC-2.6 | Le panic de **n'importe quel thread** termine **le processus** en mode `run` (`panic: <message>`, code `1`, STD-18.5) ; en mode `test`, le runner capture l'*unwind* et marque le test en échec. Il n'y a ni `catch_unwind` utilisateur, ni déroulement observable (Q-EXPR-1). | MUST |
| CONC-2.7 | Aucune garantie sur le nombre de cœurs utilisés ni sur l'ordonnancement des threads. | MUST |

```text
// NÉGATIF — affirmations interdites dans un rapport :
// « Ravum est data-race free (PROVED) »
// « le profil rt garantit le temps réel »
// « Send/Sync vérifiés »
// Toute mention doit être UNKNOWN / UNSUPPORTED / EXPERIMENTAL.
```

## 3. Interaction avec les autres invariants

| ID | Règle | Niveau |
|---|---|---|
| CONC-3.1 | **INV-05** (thread temps réel) n'est pas applicable à un programme Core 1.0 qui n'a pas de callback audio : aucun appel audio, aucune allocation en callback, aucune activation à chaud (`UNSUPPORTED`, T8). | MUST |
| CONC-3.2 | Le profil `rt` vérifie l'**absence d'effets interdits** dans le graphe d'appels, pas l'absence de préemption, d'interruption ou de défaut de cache. Un rapport `rt` ne doit jamais contenir `CERTIFIED`. | MUST |
| CONC-3.3 | Aucun mécanisme de « scheduler annulable » n'existe dans le langage ; l'annulation et le rejet de résultats périmés sont des propriétés du **daemon** (hors périmètre `spec/language/`). | MUST |
| CONC-3.4 | Un programme sans `thread::spawn`, `env::args`, `env::var`, `io::read_line`, `fs` ni `net` produit la même sortie à chaque exécution. Avec des threads, l'entrelacement n'est pas spécifié : la sortie n'est reproductible que si elle n'en dépend pas. | MUST |

## 4. Conditions d'extension future

Une extension concurrence ne peut être envisagée que si **toutes** les conditions suivantes sont réunies et **ratifiées par décision humaine**. Pour la phase 1, la décision humaine est D-18-2 et D-19-3 ; l'état de chaque condition est dans la colonne de droite.

| # | Condition | Prérequis | État phase 1 (2026-11-19) |
|---|---|---|---|
| 1 | Un cas d'usage réel et mesuré justifie le coût (P0-I, INV-10) | étude, pas préférence | décision du responsable (D-18-2), cas T6/T9 |
| 2 | Un **modèle mémoire** est spécifié (ordre, atomicité, barrières) | ADR-008 accepté | sans état partagé, seul l'ordre des canaux existe (CONC-2.3) ; ADR-008 reste `PROPOSED` |
| 3 | La propriété de transfert est définie sans traits (équivalent structurel de `Send`) | [`types.md`](types.md) Q-TYP-5 | **fait** : STD-18.1 |
| 4 | Les règles d'emprunt sont **étendues** au-delà de l'instruction (sinon aucune synchronisation n'est exprimable) | [`ownership.md`](ownership.md) Q-OWN-3 | non requis : aucune référence ne traverse un thread |
| 5 | `unsafe` et FFI restent **absents** ou sont audités séparément | ADR-009 | inchangé : le runtime de threads n'a pas d'`unsafe` |
| 6 | Un oracle indépendant existe (INV-12) et la conformance est différentielle | [`core-machine.md`](core-machine.md) | la machine de référence refuse `thread` (hors cœur pur) ; corpus de conformance |
| 7 | Le hard-RT éventuel est un **programme séparé** avec plateforme qualifiée | ADR-006/024 | non concerné |
| 8 | La compatibilité est versionnée (édition `core-1.x` → `core-2.0`) | [`compatibility.md`](compatibility.md) | avant la 1.0 : aucune version publiée n'est cassée |

| ID | Règle | Niveau |
|---|---|---|
| CONC-4.1 | Aucune extension concurrence ne peut être introduite comme « expérimentale » sans les conditions 1–3 : elle changerait le modèle de sûreté. | MUST |
| CONC-4.2 | Une extension **MUST NOT** affaiblir les garanties existantes (non-échappement des références, absence d'`unsafe fn` et de pointeurs bruts ; les blocs `unsafe` et la FFI scalaire restent des frontières explicites, pas des capacités de concurrence). | MUST |
| CONC-4.3 | Le groupe `async` est produit depuis la phase 2 de RFC-E11-CONCURRENCY (D-19-3, D-19-9, §6) ; `rt` reste réservé, sans feuille de route implicite. | MUST |
| CONC-4.4 | Toute API de concurrence future devra être **dans `std`**, versionnée, et refusée `fail-closed` sur une plateforme non supportée. | MUST |


## 6. `async fn`, `.await` et `std::task` (phase 2, D-19-9)

```rv
async fn double(x: i64) -> i64 effects(async) { x * 2 }

async fn run() -> i64 effects(async) {
    task::spawn(worker());          // tâche en file, exécutée en FIFO
    task::yield_now().await;        // laisse passer les autres tâches prêtes
    double(21).await
}

fn main() -> () effects(io, async) {
    let r: i64 = task::block_on(run());
    io::println(&r.to_string());
}
```

| ID | Règle | Niveau |
|---|---|---|
| CONC-6.1 | L'appel d'une `async fn` déclarée `-> T` rend un `Future<T>` sans rien exécuter ; `f.await` sur un `Future<T>` rend `T`. `.await` hors d'une `async fn` est `E-CONC-AWAIT-OUTSIDE` ; sur un autre type, `E-TYP-MISMATCH`. Un `Future<T>` n'est ni `Copy` ni clonable : `.await` le consomme. | MUST |
| CONC-6.2 | `task::block_on(f)` exécute `f` jusqu'à son terme, puis chaque tâche lancée par `task::spawn` ; à chaque tour, `f` est sondé d'abord, puis chaque tâche une fois, dans l'ordre de lancement. L'ordre d'exécution ne dépend que du programme, jamais du temps. | MUST |
| CONC-6.3 | Une `async fn` ne lie aucune référence : ni paramètre, ni `let`, ni motif de type référence, ni résultat référence (`E-CONC-REF-ACROSS-AWAIT`). Aucun emprunt ne peut donc être vivant à un `.await`. | MUST |
| CONC-6.4 | Toute `async fn` et toute fonction de `std::task` portent l'effet `async`, qui se propage comme les autres ; les profils `pure` et `rt` le refusent (`E-EFF-PROFILE`). | MUST |
| CONC-6.5 | Une `async fn` est une fonction libre de module, non générique et non `test` ; une méthode `async` ou un bloc `async { }` sont `E-UNSUPPORTED-ASYNC`. | MUST |
| CONC-6.6 | `task::spawn(f: Future<()>)` hors d'un `task::block_on` en cours panique ; `task::yield_now()` rend un futur en attente une seule fois. L'exécuteur est mono-thread : une tâche qui appelle une fonction bloquante (canal, `net::read_line`, `http::*`) bloque toutes les autres ; les E/S asynchrones sont celles de CONC-6.7. | MUST |
| CONC-6.7 | `net::accept_async(&TcpListener)`, `net::connect_async(&str)`, `net::read_line_async(&mut TcpStream)` et `net::write_str_async(&mut TcpStream, &str)` rendent un `Future` (effets `net`, `async`, `alloc`) qui n'emprunte pas son argument : il partage le socket. Accepter, lire et écrire ne bloquent jamais l'exécuteur : une opération non prête rend la main aux autres tâches. La connexion elle-même (`connect_async`) est établie de façon bloquante au premier sondage. Les fonctions bloquantes et asynchrones lisent le même tampon : aucun octet n'est perdu de l'une à l'autre. Quand toutes les tâches attendent un socket, l'exécuteur dort 1 ms par tour au lieu de tourner à vide. | MUST |

## 5. Questions ouvertes

- **Q-CONC-1** — Faut-il supprimer les groupes d'effets `async` et `rt` de la liste tant qu'aucune sémantique n'existe, plutôt que de les réserver ? Risque : une future extension non cassante devient impossible. Recommandation : les garder réservés et **documenter qu'ils ne prouvent rien**.
- **Q-CONC-2** — Le déterminisme total (CONC-3.4) n'est vrai qu'en l'absence d'entrées externes. Faut-il un mode `--hermetic` qui refuse `env`/`fs`/`read_line` pour garantir la reproductibilité des tests ? Non prévu par le design.
- **Q-CONC-3** — Un futur modèle mono-thread mais asynchrone (coopératif) serait bien moins coûteux qu'un modèle multi-thread. Aucune décision ; à considérer avant toute implémentation de threads.
- **Q-CONC-4** — Le profil `rt` est vérifié statiquement mais son intitulé peut induire en erreur. Recommandation : renommer l'affichage en `rt-static (EXPERIMENTAL)` dans le CLI et le JSON.
