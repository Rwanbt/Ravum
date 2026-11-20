# RFC-DSP-ABI-2 — État DSP libre pour les blocs `.rv` chargés par l'hôte

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-DSP-ABI-2` |
| Statut | **PROPOSED** — rédigée sur décision V2-D1 (2026-11-20) ; **aucune implémentation avant ratification** |
| Date | 2026-11-20 |
| Auteur | agent de développement (chantier V2-L5) |
| Gate | T8 (écart « État DSP limité à `{ z1, z2 }` (ABI 2 non ratifiée) ») |
| Reprend | [`RFC-E11-DSP-BLOCK`](RFC-E11-DSP-BLOCK.md) §1 point 3 et §6 point 1, [`RFC-RV-DSP-BLOCK`](RFC-RV-DSP-BLOCK.md) |
| Portée | `ravum-protocol` (`dsp.rs`), `ravum-patch-host`, `ravum-lang-codegen` (`--emit dsp-block`), `ravum-lang-sema` (`E-DSP-STATE-TYPE`), `ravum-dsp-harness`, `spec/language/reload.md` |

> **Statut de preuve.** Rien n'est implémenté. Les chiffres cités sont ceux
> des rapports existants, pour la configuration qu'ils déclarent.

## 1. Problème

Deux voies chargent aujourd'hui un bloc DSP écrit en `.rv` :

| Voie | Forme de l'état | Où |
|---|---|---|
| **JIT** (`ravum-lang-jit::hot`, T16) | `struct State` `.rv` **libre** (champs `Copy`), possédée par le runtime, digest HR-1.1, `migrate` (HR-1.7) | `docs/reports/T16-NATIVE-HOT-SWAP.md` §7, §9 |
| **`cdylib` via `rustc`** (`build --emit dsp-block`, hôte `ravum-patch-host`) | `DspState { z1: f32, z2: f32 }` **fixe**, `#[repr(C)]` dans `ravum-protocol` ; tout autre schéma est refusé (`E-DSP-STATE-TYPE`) | `crates/ravum-protocol/src/dsp.rs`, `DSP_STATE_SCHEMA_V1_TEXT` |

Un filtre d'ordre 4, un délai, un oscillateur à plusieurs phases ne tiennent
pas dans deux `f32`. La voie `cdylib` (celle qui produit un artefact natif
distribuable, et celle de `examples/dsp-gain-iir`) ne peut donc pas porter
les DSP que la voie JIT accepte déjà. RFC-E11-DSP-BLOCK §1 point 3 l'avait
prévu ; il a été repoussé parce qu'il change l'ABI.

## 2. Proposition

Une **ABI DSP 3** (le numéro 2 est déjà celui de `DSP_ABI_VERSION`, qui
couvre l'état fixe actuel ; le titre « ABI DSP 2 » du registre désigne cette
évolution). L'état n'est plus une struct de `ravum-protocol` : c'est un
**tampon d'octets opaque** que l'hôte possède, dont la taille, l'alignement
et le schéma sont **déclarés par le module** et vérifiés avant tout appel.

### 2.1 Exports du module

| Symbole | Type C | Rôle |
|---|---|---|
| `ravum_abi_version` | `u32` | `3` |
| `ravum_state_schema` | `*const c_char` | digest HR-1.1 (inchangé) |
| `ravum_state_layout` | `extern "C" fn() -> RavumStateLayout` | `{ size: u32, align: u32 }` |
| `ravum_state_init` | `extern "C" fn(state: *mut u8)` | écrit l'état initial (`fn init() -> State` du `.rv`) |
| `ravum_process_block` | `extern "C" fn(state: *mut u8, input: *const f32, output: *mut f32, frames: u32)` | traite un bloc |
| `ravum_state_migrate` (optionnel) | `extern "C" fn(old: *const u8, new: *mut u8)` | HR-1.7 : `migrate(old: OldState) -> State`, schéma d'origine dans `ravum_state_migrate_from` (digest) |

`DSP_SIGNATURE` devient `block:f32->f32+state:opaque:v3`.

### 2.2 Règles

| ID | Règle proposée | Niveau |
|---|---|---|
| DSP3-1 | L'état est la `struct State` du module : champs `f32`/`f64`/`i32`/`u32`/`bool`, tableaux de taille fixe de ceux-ci, et structs imbriquées de ces types (DSP-1.2 étendu aux structs) ; tout autre type est `E-DSP-STATE-TYPE`. Pas de référence, pas de `String`/`Vec`, pas de type générique. | MUST |
| DSP3-2 | La struct générée côté Rust est `#[repr(C)]` ; `size`/`align` sont ceux de ce layout. Le digest HR-1.1 couvre les noms, types et ordre des champs ; le layout `repr(C)` en est une fonction, donc deux modules au même digest ont le même layout (à vérifier par test : DSP3-T3). | MUST |
| DSP3-3 | L'hôte alloue le tampon d'état **hors du thread audio**, au chargement, avec `size`/`align` du module ; `size == 0` est permis ; `size > 64 KiB` ou `align > 64` est refusé (`RejectReason::StateLayout`). | MUST |
| DSP3-4 | Remplacement **au même digest** : le nouveau module reçoit le même tampon, l'état est conservé sans copie (comme ABI 2). | MUST |
| DSP3-5 | Remplacement **à digest différent** : accepté seulement si le nouveau module exporte `ravum_state_migrate` dont `ravum_state_migrate_from` égale le digest courant. La migration s'exécute **côté éditeur** (hors thread audio) sur une copie de l'état courant prise à un point sûr ; le thread audio n'adopte que le résultat (même schéma que V1-M1, T16 §9). Sinon `RejectReason::StateSchemaChanged` ; l'ancien bloc continue. | MUST |
| DSP3-6 | Aucune allocation, libération, migration ni compilation dans le rappel audio (INV-05) ; l'ancien tampon d'état est libéré hors du thread audio. | MUST |
| DSP3-7 | Un hôte ABI 2 refuse un module ABI 3 et inversement (`AbiVersionMismatch`) ; aucune réinterprétation silencieuse. | MUST |
| DSP3-8 | ABI 2 reste accepté (`DspState { z1, z2 }`) : un module ABI 2 existant continue de se charger tel quel (COMPAT). Son retrait demande une décision séparée. | MUST |
| DSP3-9 | Le code `unsafe` nouveau (lecture du layout, appel à travers `*mut u8`) est confiné dans `ravum-patch-host` et le wrapper généré (DSP-1.3), listé dans `docs/ffi/UNSAFE-INVENTORY.md` et ajouté au dossier `docs/audit/UNSAFE-AUDIT-DOSSIER.md`. | MUST |
| DSP3-10 | `process_sample(&mut State, f32) -> f32` reste la forme source ; le wrapper généré boucle sur `frames` (pas de taille de bloc fixe). | MUST |

### 2.3 Exemple

```rv
struct State { z: [f32; 4], gain: f32 }

fn init() -> State effects() { State { z: [0.0, 0.0, 0.0, 0.0], gain: 0.5 } }

fn process_sample(state: &mut State, input: f32) -> f32 effects() {
    let y: f32 = input * state.gain + state.z[3];
    state.z[3] = state.z[2];
    state.z[2] = state.z[1];
    state.z[1] = state.z[0];
    state.z[0] = input;
    y
}
```

## 3. Tests prévus (positifs et négatifs)

- **DSP3-T1** différentiel : 10 signaux fixés, état à 4 champs et à tableau,
  sortie `cdylib` = JIT = interpréteur bit à bit.
- **DSP3-T2** 10 000 blocs, 20 remplacements au même digest : 0 allocation
  / 0 libération dans le rappel, état conservé.
- **DSP3-T3** même digest ⇒ même `size`/`align` (programmes générés).
- **DSP3-T4** digest différent sans `migrate` → refus, ancien bloc continue ;
  avec `migrate` → état migré bit à bit, adopté au bloc suivant ; trap dans
  `migrate` → refus.
- **DSP3-T5** négatifs : `String` dans l'état (`E-DSP-STATE-TYPE`), état
  > 64 KiB (`StateLayout`), module ABI 3 sur hôte ABI 2 et l'inverse.
- **DSP3-T6** non-régression : tous les modules ABI 2 existants (fixtures,
  `tests/dsp_block.rs`) passent inchangés.

## 4. Alternatives écartées

- **Garder `{ z1, z2 }` et ajouter des variantes fixes** (`DspState4`, …) :
  explosion combinatoire, pas de DSP arbitraire.
- **L'état possédé par le module** (`static mut`) : la conservation au
  remplacement deviendrait une copie entre modules sans schéma commun ;
  contraire au choix « l'hôte possède l'état » de `dsp.rs`.
- **Sérialiser l'état** (serde) à chaque remplacement : allocation, et coût
  sans gain au même digest.
- **Réserver le DSP libre au JIT** : la voie `cdylib` resterait incapable de
  produire l'artefact de `examples/dsp-gain-iir` ; T8 resterait `PARTIAL`.

## 5. Questions au responsable

1. Ratifier ABI 3 (état opaque déclaré par le module) ?
2. Limites DSP3-3 (64 KiB, alignement 64) : acceptables ?
3. Migration côté éditeur pour la voie `cdylib` aussi (DSP3-5), comme V1-M1 ?
4. Garder ABI 2 sans date de retrait (DSP3-8) ?
