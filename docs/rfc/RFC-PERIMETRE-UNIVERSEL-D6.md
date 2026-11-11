# RFC — Périmètre « universel » de Core 1.0 : réseau, async, threads, hot-state

> **Statut : `PROPOSED`** · **Date : 2026-11-10** · **Portée : Ravum Core 1.0**
> **Décision liée : D6** (`docs/roadmap/CORE-1.0-GATE-MATRIX.md` §4)
> **Ce document ne ratifie rien** et n'autorise aucune implémentation.

## 1. Problème

L'objectif déclaré du projet est un **langage de programmation universel**
(`README.md`, `docs/adr/ADR-000-…`). Or Core 1.0 est explicitement **synchrone,
mono-thread et sans réseau** : le parseur refuse `async`/`await`
(`E-UNSUPPORTED-ASYNC`), la sémantique refuse la déclaration d'un effet non
implémenté (`E-UNSUPPORTED-EFFECT`, qui cite `net`, `unsafe`, `ffi`, `async`), et
la matrice de gates laisse T6 (réseau/async/threads) et T8 (hot-state)
`PARTIAL`/absents. Le mot « universel » du titre ne décrit donc pas le périmètre
livré : c'est la tension que D6 doit trancher.

Deux mauvaises réponses sont à écarter d'emblée :

- **Ajouter les fonctionnalités pour honorer le titre** : c'est une expansion de
  périmètre non contrôlée, qui diluerait les preuves existantes (conformance,
  couverture, mutation, Miri, repro) sans les étendre.
- **Retirer le mot « universel »** : c'est une suppression arbitraire de
  l'objectif produit, interdite par les règles du projet.

## 2. Ce qui existe, ce qui est refusé, ce qui n'existe pas

État réel au commit de cette RFC (voir la matrice de gates pour les mesures) :

| Élément | État |
|---|---|
| Effets déclarés dans la spec | `pure`, `io`, `fs`, `env`, `process`, `ffi`, `unsafe` (liste fermée de `spec/language/effects.md`) |
| Effets **reconnus mais refusés** | `net`, `async` → `E-UNSUPPORTED-EFFECT` |
| Syntaxe `async`/`await` | refusée au parseur (`E-UNSUPPORTED-ASYNC`) |
| Threads | aucune surface (pas de `spawn`, pas de `Send`/`Sync`, pas de `Arc`) |
| Réseau | aucun module `std::net` ; seuls `io`, `fs`, `string`, `math`, `env`, `process`, `std` existent |
| Hot-state (remplacement de code à chaud) | **aucune** implémentation ; le daemon/overlay existe côté plateforme Rust, pas côté langage |
| Contrainte temps réel | énoncée comme **interdiction** (pas d'allocation/blocage dans un callback audio), non outillée par le langage |

Conséquence honnête : **aucun programme Ravum ne peut aujourd'hui servir un port
réseau, ni exécuter deux tâches concurrentes, ni se recharger à chaud.** Ce n'est
pas un défaut caché ; c'est le périmètre déclaré.

## 3. Alternatives

### Option 1 — Périmètre 1.0 explicite, extensions reportées en 1.x

- Core 1.0 reste synchrone/mono-thread/sans réseau ; `E-UNSUPPORTED-EFFECT` et
  `E-UNSUPPORTED-ASYNC` deviennent la **frontière contractuelle** du langage, et
  la doc cesse d'employer « universel » sans qualificatif (« noyau universel
  visé ; sous-ensemble 1.0 synchrone »).
- **Coût** : rédaction (spec + README + matrice). Aucun code.
- **Risque** : le titre reste optimiste ; mitigé par une phrase explicite en tête
  de README et par T6/T8 maintenus `PARTIAL`.
- **Falsification** : si un utilisateur réel a besoin d'un serveur, il ne peut pas
  l'écrire → l'option devient un cul-de-sac produit, pas une étape.

### Option 2 — Réseau bloquant minimal (`net`), sans async

- `std::net` (`TcpListener`, `TcpStream` : `accept`, `read`, `write`, `connect`),
  effet `net` distinct de `io`, appels bloquants uniquement.
- **Avantage** : pas de runtime, pas d'exécuteur, pas de threads → la chaîne de
  preuve actuelle (mono-thread) reste valide. Un serveur séquentiel est écrivable.
- **Coût** : module stdlib + effet + conformance **hermétique** (sockets sur
  `127.0.0.1` avec un port éphémère, jamais d'Internet) + tests de refus
  (`net` hors profil `core`, `E-EFF-*`).
- **Risque** : un serveur séquentiel bloque sur une connexion lente ; ce n'est pas
  une architecture serveur. À annoncer comme tel.

### Option 3 — `async`/`await` avec exécuteur déterministe mono-thread

- Surface `async fn` + `await`, abaissée vers un exécuteur **déterministe** (ordre
  des tâches fixé), sans threads ni course de données par construction.
- **Avantage** : l'effet `async` est déjà nommé par la spec ; le déterminisme rend
  la qualification possible (rejeu, pas de flakiness).
- **Coût** : le plus lourd des trois en sémantique (types `Future`, `await` hors
  contexte, effets transitifs, `?` dans `async`), plus un second modèle
  d'exécution à qualifier (scheduling, annulation, ordre de réveil).
- **Risque** : sans threads, `async` n'apporte que la concurrence d'E/S ; le gain
  ne se matérialise qu'avec l'option 2 en même temps.

### Option 4 — Threads avec passage de messages uniquement

- `spawn`, canaux, **aucun état mutable partagé** (pas de `Arc<Mutex<T>>` dans Core).
- **Coût** : surface de types + runtime + qualification de la concurrence (TSan,
  stress, reproductibilité des traces) — le job TSan est aujourd'hui désactivé et
  documenté `NOT_APPLICABLE` précisément parce qu'aucun chemin concurrent n'existe.
- **Risque** : réactive un domaine de preuve entier (concurrence) pour un langage
  dont l'ownership est mono-thread ; c'est un projet à part entière.

### Option 5 — Hot-state (remplacement à chaud) : conception seulement

- Écrire l'invariant et le modèle de ressources (qui possède quoi pendant un
  swap, quelles allocations sont interdites dans un callback), **sans**
  implémentation, et le rattacher aux règles RT existantes.
- **Coût** : spec + revue. **Risque** : une spec sans implémentation peut être lue
  comme une promesse ; à marquer `PROPOSED` et non `MUST`.

## 4. Recommandation (non contraignante)

Une séquence, pas une option unique :

1. **Maintenant** : option 1 pour Core 1.0 (frontière contractuelle explicite,
   README honnête). T6/T8 restent `PARTIAL` et nomment D6.
2. **Pilote 1.x** : option 2 d'abord (réseau bloquant, hermétique, mono-thread),
   car elle est testable avec la chaîne de preuve **existante** ; puis option 3
   seulement si le besoin d'E/S concurrentes est démontré par un cas réel. Dans
   ce cas, option 2 et 3 ensemble, jamais 3 seule.
3. **Option 4** : non recommandée avant qu'un chemin concurrent réel et outillé
   existe (réactivation de TSan comme condition, pas comme conséquence).
4. **Option 5** : conception seulement, explicitement non normative.

## 5. Ce qu'un humain doit décider

1. La frontière 1.0 (option 1 acceptée ou non) et la formulation publique.
2. L'ordre et le périmètre des pilotes 1.x (sous-ensemble exact, hermétique ou non).
3. La condition de réactivation de TSan si les threads sont envisagés.

## 6. Ce que cette RFC ne fait pas

- Elle n'ajoute ni effet, ni module, ni syntaxe.
- Elle ne prétend pas que `E-UNSUPPORTED-EFFECT` est une décision définitive.
- Elle ne cite aucun débit réseau ni aucune latence : rien de tel n'est mesuré.
