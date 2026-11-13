# RFC-RV-DSP-BLOCK-1 — `ravum-lang build --emit dsp-block` : relier le langage au hot swap DSP

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-RV-DSP-BLOCK-1` |
| Statut | **PROPOSED** — `PENDING_HUMAN_APPROVAL` (ABI, `unsafe` généré, extension du protocole) |
| Date | 2026-11-13 |
| Auteur | agent de développement (session GOAL) |
| Portée | `crates/ravum-lang-codegen` (wrapper), `crates/ravum-lang-cli` (`--emit dsp-block`), `crates/ravum-protocol` (schéma d'état), `crates/ravum-patch-host` (consommation) |
| Décision attendue | ratifier l'ABI, le digest de schéma d'état et le confinement de l'`unsafe` généré |

> **Ce que cette RFC ne promet pas.** Ni « temps réel garanti », ni « hard-RT ».
> Elle relie un compilateur à un hôte existant qui a mesuré 0 allocation au
> callback sur des tests ; elle ne qualifie pas une plateforme audio.

## 1. État de l'ABI de bloc existante (lu, pas supposé)

`crates/ravum-patch-host/src/block.rs` :

```rust
pub const DSP_EXPORT: &str = "ravum_process_block";            // ravum-protocol/src/dsp.rs
pub type BlockFn = unsafe extern "C" fn(
    input: *const f32,
    output: *mut f32,
    frames: u32,
    state: *mut DspState,
);
pub struct DspState { pub z1: f32, pub z2: f32 }               // ravum-protocol/src/dsp.rs
```

L'hôte charge un `cdylib`, résout `ravum_process_block`, garde l'ancien module
vivant pendant un crossfade optionnel (zéro allocation sur le callback,
`MAX_FADE_FRAMES = 512`) et **refuse** un module dont l'ABI ne correspond pas
(`validate_loaded`, `evaluate_gates`, `RejectReason`). L'état est possédé par
l'hôte et passé par pointeur ; aucun état n'est migré.

Aujourd'hui, seuls des modules Rust écrits à la main remplissent ce contrat.
Un programme `.rv` ne peut pas produire de `cdylib` ni exposer cette entrée.

## 2. Proposition

### 2.1 Source Ravum

Un module DSP est un package binaire dont une fonction porte le profil `rt` et
une signature convenue :

```rv
// src/main.rv (ou lib) — forme candidate, à ratifier avec la RFC-RT-TRAPS-1
struct DspState { pub z1: f32, pub z2: f32 }

fn process_block(input: &[f32], output: &mut [f32], state: &mut DspState) effects(rt) {
    let mut i: usize = 0;
    while i < input.len() {
        state.z1 = 0.9 * state.z1 + 0.1 * input[i];
        output[i] = state.z1;
        i += 1;
    }
}
```

Règles :

- la fonction désignée est choisie par `--dsp-entry <nom>` (défaut :
  `process_block`) ; une seule fonction ;
- profil **`rt` obligatoire** : le build refuse une entrée `core`/`pure`
  (`E-EFF-PROFILE`), donc les traps implicites doivent être fermés d'abord
  (RFC-RT-TRAPS-1) ;
- pas d'allocation dans le corps : les effets `alloc`/`io`/`fs`/`net`/`panic`
  sont interdits par le profil ;
- la signature est vérifiée par le checker **et** par le codegen
  (`CodegenError` si contourné).

### 2.2 Wrapper généré (jamais écrit en `.rv`)

Le codegen produit un `cdylib` Rust dont le seul `unsafe` est le wrapper :

```rust
#![deny(unsafe_code)]

#[repr(C)]
struct RvState { z1: f32, z2: f32 }

#[no_mangle]
pub extern "C" fn ravum_process_block(
    input: *const f32, output: *mut f32, frames: u32, state: *mut RvState,
) {
    // SAFETY: l'hôte garantit des buffers de `frames` f32 valides et un état
    // aligné de ce layout ; le wrapper ne retient aucun pointeur.
    #[allow(unsafe_code)]
    unsafe {
        debug_assert!(!input.is_null() && !output.is_null() && !state.is_null());
        let input = std::slice::from_raw_parts(input, frames as usize);
        let output = std::slice::from_raw_parts_mut(output, frames as usize);
        let state = &mut *state;
        rv_process_block(input, output, state);   // le corps Ravum, Rust sûr
    }
}

#[no_mangle]
pub extern "C" fn ravum_state_schema() -> u64 { /* FNV-1a 64 du schéma */ }
#[no_mangle]
pub extern "C" fn ravum_state_schema_text() -> *const u8 { /* "z1:f32,z2:f32;" */ }
```

Contraintes :

- le corps Ravum reste du **Rust sûr** (aucun pointeur, aucune API `unsafe`) ;
- l'`unsafe` est **local** au wrapper, avec `#[allow(unsafe_code)]` et un
  commentaire `// SAFETY:` ; le reste du fichier garde `deny` (comme la FFI,
  design §3.7 bis FFI-6) ;
- aucune allocation, aucun verrou, aucun `Drop` coûteux dans le wrapper : il ne
  fait que convertir des pointeurs en slices et appeler le corps ;
- une panique ne traverse **jamais** la frontière : le build `dsp-block` utilise
  `panic = "abort"` et le corps `rt` ne peut pas paniquer (RFC-RT-TRAPS-1).

### 2.3 Digest de schéma d'état et refus de swap

- Le codegen calcule un **FNV-1a 64** sur le texte canonique du schéma
  (`nom:type` dans l'ordre de déclaration, séparés par `,`, terminés par `;`).
  Ce n'est **pas** une primitive cryptographique : c'est une empreinte de
  layout, sans dépendance nouvelle (R5) ; le texte est aussi exporté pour
  l'audit.
- L'hôte compare le digest du nouveau module à celui du module actif :
  - digest identique ⇒ swap autorisé ;
  - digest différent ⇒ **swap refusé**, l'ancien module reste actif
    (`RejectReason::StateSchemaMismatch`), ce qui matérialise **INV-03** ;
- **aucune migration d'état** dans cette verticale. Un changement de schéma
  exige un nouveau build de l'hôte, pas un patch à chaud.
- Le schéma d'état doit correspondre exactement au `DspState` de l'hôte
  (`z1: f32, z2: f32`) pour un swap ; c'est volontairement étroit.

## 3. Alternatives et coûts

| Option | Coût | Verdict |
|---|---|---|
| (a) Garder le hot swap réservé au Rust | nul | ne relie pas les deux lignes (constat C2 de l'audit) |
| (b) `cdylib` sans digest de schéma | faible | swap silencieux incompatible, viole INV-03 |
| (c) Cette RFC | moyen : wrapper généré, flag CLI, extension protocole, tests | **retenu** |
| (d) Migration d'état | recherche | hors périmètre |

## 4. Validation exigée

- build d'un exemple `.rv` en `--emit dsp-block`, chargement par l'hôte réel et
  rendu d'un buffer identique à la version Rust de référence (tolérance
  numérique documentée) ;
- **swap réussi pendant le rendu**, avec **0 allocation et 0 désallocation**
  sur le callback (réutilise l'instrumentation de `rt_lifecycle.rs`) ;
- **swap refusé** quand le schéma d'état change (digest différent), ancienne
  version conservée et toujours fonctionnelle ;
- module incompilable ⇒ ancienne version conservée (fail-closed) ;
- cas négatifs : entrée absente, deux entrées, signature fausse, profil `core`,
  allocation dans le corps, panic possible ;
- `cargo test -p ravum-patch-host` + `cargo test -p ravum-lang-cli` verts, aucune
  allocation mesurée au callback.

## 5. Risques

- **ABI** : `repr(C)` sur `RvState`, alignement et ordre des champs ; vérifié par
  le test de swap réel, pas par la seule compilation.
- **Plateforme** : `cdylib` Windows/MSVC et Linux ; macOS hors qualification
  (`NOT_RUN`).
- **Sûreté** : le wrapper `unsafe` est la seule surface non prouvée ; il est
  local, commenté, généré et testé. Un pointeur invalide fourni par un hôte
  défectueux reste possible, comme pour toute ABI C.
- **Temps réel** : « 0 allocation au callback sur ce test » n'est pas une
  garantie hard-RT ; aucun WCET n'est mesuré.

## 6. Décision demandée

1. Ratifier la signature, le profil `rt` obligatoire et le wrapper généré.
2. Ratifier le digest de schéma (FNV-1a 64 + texte exporté) comme frontière de
   compatibilité, ou exiger une empreinte cryptographique (dépendance nouvelle).
3. Confirmer qu'aucune migration d'état n'est attendue en Core 1.0.
