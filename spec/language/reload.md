# Ravum Core 1.0 — Rechargement à chaud avec préservation d'état

> **Statut : DRAFT — CANDIDATE, non ratifiée.**
> **Version : `core-1.0-draft.1`** · **Date : 2026-11-19** · **Portée : Ravum Core 1.0** · **Source de conception : [`RFC-E11-HOT-RELOAD`](../../docs/rfc/RFC-E11-HOT-RELOAD.md) (`PROPOSED`), modèle fixé par [D-19-5](../../docs/decision/DECISIONS-2026-11-19.md).**
>
> Ce document décrit `ravum-lang watch --reload`. Il ne promet ni migration
> arbitraire, ni remplacement de fonctions au milieu d'un appel, ni
> préservation de threads, de connexions ou de handles : l'état préservé est
> **une valeur déclarée**, de type fixé.

## 1. Programme rechargeable

Un programme rechargeable déclare, dans son **module racine** :

```rv
struct State { count: i64 }

fn init() -> State effects() { State { count: 0 } }
fn step(state: State, line: String) -> State effects() { State { count: state.count + 1 } }
// optionnel :
fn migrate(old: Before) -> State effects() { ... }

fn main() -> () effects() { ... }   // exigé comme pour tout binaire ; non appelé par l'hôte
```

| ID | Règle | Niveau |
|---|---|---|
| HR-1.1 | Le **schéma d'état** est le type `State` développé : pour chaque type utilisateur atteint, dans l'ordre de première apparition (`#0` est `State`), sa forme (`struct` ou `enum`), les noms de ses champs et de ses variantes dans l'ordre de déclaration et le type de chaque composant. Les types utilisateur sont désignés par leur rang, pas par leur nom. Son digest est le SHA-256 de ce texte canonique ; il est inscrit dans le module construit. | MUST |
| HR-1.2 | Un remplacement de module n'a lieu **qu'entre deux** appels de `step`, jamais pendant : l'hôte traite les événements et les remplacements l'un après l'autre. | MUST |
| HR-1.3 | Un module dont le digest diffère de celui du module courant est **refusé**, sauf si sa fonction `migrate` prend exactement le schéma courant (HR-1.7) ; l'ancien module continue, sans conversion implicite. | MUST |
| HR-1.4 | Un module qui ne se construit pas (diagnostic, contrat, `rustc`) ou ne démarre pas est refusé ; l'ancien module continue (fail-closed). | MUST |
| HR-1.5 | Une panique dans `step` laisse l'état d'avant l'appel. Si un module précédent est disponible, l'hôte y revient et lui rejoue l'événement ; sinon l'événement est abandonné et l'état gardé, avec un message. | MUST |
| HR-1.6 | `State` et les types qu'il contient sont des structs ou des enums utilisateur **non génériques**, composés de scalaires, `String`, `Vec`, `Option`, `Result`, tuples et tableaux de taille fixe (`E-HR-STATE-TYPE`). Une référence ou une ressource (`TcpListener`, `TcpStream`, `JoinHandle`, canaux, `CHandle`, requêtes HTTP) est refusée (`E-HR-STATE-RESOURCE`). | MUST |
| HR-1.7 | `fn migrate(old: T) -> State`, optionnelle, prend un autre type utilisateur `T`. Le module qui la déclare est accepté quand le schéma de `T` égale le schéma courant : l'état courant est lu comme un `T` et converti. Une panique dans `migrate` refuse le module. | MUST |
| HR-1.8 | `init`, `step` et `migrate` ont exactement les signatures ci-dessus (`E-HR-ENTRY`). `ravum-lang check --reload` vérifie ce contrat sans construire. | MUST |

## 2. Transport de l'état

| ID | Règle | Niveau |
|---|---|---|
| HR-2.1 | L'état traverse un remplacement sous sa **forme canonique** : une ligne de texte, champs dans l'ordre du schéma, produite par l'ancien module et relue par le nouveau. Les flottants y sont écrits par leurs bits : la relecture est exacte. | MUST |
| HR-2.2 | Chaque module est un processus séparé ; aucun pointeur ni disposition mémoire n'est partagé entre deux modules, et l'hôte n'exécute aucun code `unsafe` ni chargement dynamique pour un remplacement. | MUST |
| HR-2.3 | Chaque ligne de l'entrée standard de l'hôte est un événement, passé à `step` comme `line`. Quand l'entrée se ferme, l'hôte écrit `reload: final state <forme canonique>` sur sa sortie d'erreur et s'arrête avec le code 0. Les messages de l'hôte vont sur la sortie d'erreur ; la sortie standard est celle du programme. | MUST |

## 3. Écarts avec la RFC

- l'événement est une ligne de texte (`String`), pas un enum `Event` fourni
  par l'hôte : un enum hôte demanderait une variante de prélude que le
  vérificateur ne connaît pas, pour un gain nul (la fin d'entrée remplace `Stop`) ;
- le module est un **processus** et non un `cdylib` chargé (§3 de la RFC) :
  c'est ce qui rend HR-2.2 vrai sans `unsafe` ;
- l'état d'avant chaque `step` est copié (`clone`) dans le module, pour que
  HR-1.5 tienne à tout moment, pas seulement au premier événement.

## 4. Qualification

`crates/ravum-lang-cli/tests/hot_reload.rs` : 10 000 événements avec 20
éditions compatibles (état final exact), schéma changé refusé, `migrate`,
panique dans `step` revenue à l'ancien module sans perte, édition qui ne
compile pas. Conformance : `types/reload-*`.
