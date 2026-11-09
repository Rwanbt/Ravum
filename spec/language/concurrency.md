# Ravum Core 1.0 — Concurrence (modèle candidat)

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-09** · **Portée : Ravum Core 1.0** · **Base : `dev` @ `5408d9b`** · **Source de conception : `docs/architecture/RAVUM-CORE-DESIGN.md` §3.6, §5.**
>
> Ce document est une **candidate**. Il **ne ratifie aucun ADR** (notamment pas ADR-008 « concurrence, synchronisation et annulation » ni ADR-024 « thread audio »). Aucune implémentation n'est revendiquée (`NOT_IMPLEMENTED`).

## 1. Décision : aucune concurrence en Core 1.0

| ID | Règle | Niveau |
|---|---|---|
| CONC-1.1 | Core 1.0 est **mono-thread** : un programme s'exécute sur un seul thread logique, séquentiellement. | MUST |
| CONC-1.2 | Il n'existe **aucune** primitive de concurrence : ni `thread`, ni `spawn`, ni `join`, ni `Mutex`, ni `RwLock`, ni `Arc`, ni `atomic`, ni canal, ni `async`/`await`, ni `select`, ni domaine parallèle. | MUST |
| CONC-1.3 | Les groupes d'effets `async` et `rt` ne sont **pas** des mécanismes d'exécution : `async` est refusé à la déclaration (`E-UNSUPPORTED-EFFECT`) et `rt` est réservé, sans sémantique ([`effects.md`](effects.md)). | MUST |
| CONC-1.4 | La seule « concurrence » possible est **externe** au langage : plusieurs processus `ravum-lang run` lancés par l'utilisateur. Ils ne partagent aucun état mémoire ; les interactions passent par le système de fichiers ou les E/S standard, hors du modèle Ravum. | MUST |
| CONC-1.5 | Aucun programme Ravum ne peut créer, lire ou écrire un état partagé entre deux threads. | MUST |

```rv
// POSITIF — un seul thread, séquentiel
fn main() {
    let mut n = 0;
    for i in 0..10 { n += i; }
    io::println(n.to_string().as_str());
}
```

```rv
// NÉGATIF — E-RES-UNKNOWN (aucune API thread)
use std::thread;
fn main() { let h = thread::spawn(f); }
// NÉGATIF — E-UNSUPPORTED-ASYNC
async fn f() { }
// NÉGATIF — E-UNSUPPORTED-EFFECT (déclaration du groupe async)
fn g() effects(async) { }
```

## 2. Ce que le langage ne promet pas

| ID | Non-promesse | Niveau |
|---|---|---|
| CONC-2.1 | Le langage **ne promet pas** « data races impossibles » comme théorème : il n'y a pas de mémoire partagée entre threads, donc la question est **vide** en Core 1.0. Aucune preuve, aucun modèle mémoire, aucun `PROVED` n'est revendiqué. | MUST |
| CONC-2.2 | Il n'existe **aucun** `Send`/`Sync`, aucun marqueur de transfert, aucune borne de concurrence. Les génériques n'ont pas de bornes ([`types.md`](types.md) TYP-11.6). | MUST |
| CONC-2.3 | Il n'existe **aucun** ordre mémoire, aucune barrière, aucune sémantique d'atomicité — pas même `Relaxed`. | MUST |
| CONC-2.4 | Aucune garantie de « fairness », de « pas de deadlock », ni de « pas de famine » : ces propriétés sont hors modèle. | MUST |
| CONC-2.5 | Aucune garantie de latence, de gigue, ni de temps réel. `rt` est **expérimental** et ne certifie **pas** le hard-RT (design §3.6, [`effects.md`](effects.md) EFF-4.6). | MUST |
| CONC-2.6 | Le panic termine **le processus** en mode `run` (aucun destructeur) ; en mode `test`, le runner capture l'*unwind* et marque le test en échec. Il n'y a ni propagation inter-thread, ni `catch_unwind` utilisateur, ni déroulement observable (Q-EXPR-1). | MUST |
| CONC-2.7 | Aucune garantie sur le nombre de cœurs utilisés : un programme Core 1.0 en utilise **un**. | MUST |

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
| CONC-3.4 | Le déterminisme d'exécution est **total** en Core 1.0 : un programme sans `env::args`/`env::var`/`io::read_line`/`fs` produit la même sortie à chaque exécution. | MUST |

## 4. Conditions d'extension future

Une extension concurrence ne peut être envisagée que si **toutes** les conditions suivantes sont réunies et **ratifiées par décision humaine** :

| # | Condition | Prérequis |
|---|---|---|
| 1 | Un cas d'usage réel et mesuré justifie le coût (P0-I, INV-10) | étude, pas préférence |
| 2 | Un **modèle mémoire** est spécifié (ordre, atomicité, barrières) | ADR-008 accepté |
| 3 | La propriété de transfert est définie sans traits (équivalent structurel de `Send`) | [`types.md`](types.md) Q-TYP-5 |
| 4 | Les règles d'emprunt sont **étendues** au-delà de l'instruction (sinon aucune synchronisation n'est exprimable) | [`ownership.md`](ownership.md) Q-OWN-3 |
| 5 | `unsafe` et FFI restent **absents** ou sont audités séparément | ADR-009 |
| 6 | Un oracle indépendant existe (INV-12) et la conformance est différentielle | [`core-machine.md`](core-machine.md) |
| 7 | Le hard-RT éventuel est un **programme séparé** avec plateforme qualifiée | ADR-006/024 |
| 8 | La compatibilité est versionnée (édition `core-1.x` → `core-2.0`) | [`compatibility.md`](compatibility.md) |

| ID | Règle | Niveau |
|---|---|---|
| CONC-4.1 | Aucune extension concurrence ne peut être introduite comme « expérimentale » sans les conditions 1–3 : elle changerait le modèle de sûreté. | MUST |
| CONC-4.2 | Une extension **MUST NOT** affaiblir les garanties existantes (non-échappement des références, absence d'`unsafe`). | MUST |
| CONC-4.3 | Les groupes d'effets `async`/`rt` existent pour **réserver** la syntaxe ; ils ne constituent pas une feuille de route implicite. | MUST |
| CONC-4.4 | Toute API de concurrence future devra être **dans `std`**, versionnée, et refusée `fail-closed` sur une plateforme non supportée. | MUST |

## 5. Questions ouvertes

- **Q-CONC-1** — Faut-il supprimer les groupes d'effets `async` et `rt` de la liste tant qu'aucune sémantique n'existe, plutôt que de les réserver ? Risque : une future extension non cassante devient impossible. Recommandation : les garder réservés et **documenter qu'ils ne prouvent rien**.
- **Q-CONC-2** — Le déterminisme total (CONC-3.4) n'est vrai qu'en l'absence d'entrées externes. Faut-il un mode `--hermetic` qui refuse `env`/`fs`/`read_line` pour garantir la reproductibilité des tests ? Non prévu par le design.
- **Q-CONC-3** — Un futur modèle mono-thread mais asynchrone (coopératif) serait bien moins coûteux qu'un modèle multi-thread. Aucune décision ; à considérer avant toute implémentation de threads.
- **Q-CONC-4** — Le profil `rt` est vérifié statiquement mais son intitulé peut induire en erreur. Recommandation : renommer l'affichage en `rt-static (EXPERIMENTAL)` dans le CLI et le JSON.
