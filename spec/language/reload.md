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

## 3. Bloc DSP (RFC-E11-DSP-BLOCK)

`ravum-lang build --emit dsp-block` construit un `cdylib` chargé par l'hôte de
blocs (`ravum-patch-host`, ABI DSP 2, export `ravum_process_block`) :

```rv
struct State { z1: f32, z2: f32 }

fn process_sample(state: &mut State, input: f32) -> f32 effects() {
    let y: f32 = 0.1 * input + 0.9 * state.z1;
    state.z1 = y;
    y
}
```

| ID | Règle | Niveau |
|---|---|---|
| DSP-1.1 | `--emit dsp-block` vérifie le programme sous le profil `rt` depuis `process_sample` (pas d'allocation, pas d'effet hors `rt`) ; il n'accepte ni `--rt-entry` ni `--target` (exit 2). `ravum-lang check --dsp-block` fait la même vérification sans construire. | MUST |
| DSP-1.2 | L'état est exactement l'état possédé par l'hôte : `struct State { z1: f32, z2: f32 }`, vérifié par son schéma HR-1.1 (`E-DSP-STATE-TYPE`). `process_sample` a exactement la signature `fn(state: &mut State, input: f32) -> f32` (`E-DSP-ENTRY`). L'état initial est celui de l'hôte (zéros). | MUST |
| DSP-1.3 | Le seul `unsafe` du bloc est le module généré `rv_dsp_abi`, sous un `#[allow(unsafe_code)]` local dans un fichier `#![deny(unsafe_code)]` ; il est décrit dans `docs/ffi/SAFETY.md` §8 et n'est jamais écrit par l'utilisateur. | MUST |
| DSP-1.4 | Le bloc exporte `ravum_state_schema`, le digest HR-1.1 de son état. Un hôte construit avec `ExpectedAbi::dsp_with_state_schema()` refuse un module dont le digest diffère (`RejectReason::StateSchemaChanged`) ou qui n'en exporte pas (`MissingExport`), sans l'appeler ; l'ancien bloc continue. | MUST |
| DSP-1.5 | Le bloc appelle `process_sample` une fois par échantillon, dans l'ordre, pour toute taille de bloc donnée par l'hôte ; l'état est relu de l'hôte au début du bloc et lui est rendu à la fin. | MUST |

## 4. Écarts avec la RFC

- l'événement est une ligne de texte (`String`), pas un enum `Event` fourni
  par l'hôte : un enum hôte demanderait une variante de prélude que le
  vérificateur ne connaît pas, pour un gain nul (la fin d'entrée remplace `Stop`) ;
- le module est un **processus** et non un `cdylib` chargé (§3 de la RFC) :
  c'est ce qui rend HR-2.2 vrai sans `unsafe` ;
- l'état d'avant chaque `step` est copié (`clone`) dans le module, pour que
  HR-1.5 tienne à tout moment, pas seulement au premier événement ;
- bloc DSP : l'état n'est pas une struct `.rv` libre mais l'état `{ z1, z2 }`
  que l'hôte possède déjà (ABI DSP 2 inchangée, DSP-1.2) ; l'élargir demande
  une nouvelle version d'ABI, donc une ratification ;
- bloc DSP : une fonction par échantillon (`process_sample`) au lieu de
  `process(&mut State, &[f32; 64], &mut [f32; 64])` : aucune taille de bloc
  n'est fixée à la compilation, donc rien à refuser (la RFC DSP-1.5 devient
  sans objet).

## 5. Qualification

`crates/ravum-lang-cli/tests/hot_reload.rs` : 10 000 événements avec 20
éditions compatibles (état final exact), schéma changé refusé, `migrate`,
panique dans `step` revenue à l'ancien module sans perte, édition qui ne
compile pas. Conformance : `types/reload-*`.

`crates/ravum-lang-cli/tests/dsp_block.rs` : le bloc `.rv` égale bit à bit un
filtre de référence Rust sur 10 signaux ; 10 000 blocs avec 20 remplacements
compatibles, état porté à travers chacun, **0** allocation et **0**
désallocation mesurées sur le callback ; module au schéma différent ou sans
schéma refusé par l'hôte. Conformance : `types/dsp-block-*`,
`effects/dsp-block-allocation-refused`.
