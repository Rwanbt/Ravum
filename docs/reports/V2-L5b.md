# V2-L5b — T8 : ABI DSP 3 (RFC-DSP-ABI-2), tranche 1 : vérificateur et générateur

| Champ | Valeur |
|---|---|
| Gate | T8 |
| RFC | `RFC-DSP-ABI-2` (`ACCEPTED`, V2-R2) |
| Branche | `claude/trusting-cerf-4hhv5a` |
| Environnement | conteneur cloud Linux x86_64, rustc 1.95.0 |

## Fait (tranche 1)

- **Vérificateur** (`ravum-lang-sema::reload::dsp_contract`) : `DspContract`
  porte `abi` (2 ou 3) et `init`. État `{ z1: f32, z2: f32 }` → ABI 2,
  **inchangée** (DSP3-8). Autre état dont tous les champs sont `f32`, `f64`,
  `i32`, `u32`, `bool`, tableaux de taille fixe ou structs non génériques de
  ces types (profondeur ≤ 8) → ABI 3 (DSP3-1), qui exige `fn init() -> State`
  (`E-DSP-ENTRY` sinon). Sinon `E-DSP-STATE-TYPE`.
- **Générateur** : en ABI 3, `#[repr(C)]` sur chaque struct non générique
  (DSP3-2 : le layout est fonction du schéma) ; module `rv_dsp_abi` qui
  exporte `ravum_abi_version` = 3, `ravum_manifest_v1` (signature
  `block:f32->f32+state:opaque:v3`), `ravum_state_schema` (digest HR-1.1),
  `ravum_state_layout` (`{ size, align }`), `ravum_state_init(*mut u8)`,
  `ravum_process_block(input, output, frames, *mut u8)` (DSP3-10 : boucle par
  échantillon). Même confinement de l'`unsafe` qu'en ABI 2 (DSP-1.3).
- Vérifié à la main : `examples/dsp-gain-iir/block/process.rv` (état `{ y }`)
  se construit en `cdylib` et exporte les six symboles (`nm -D`).
- Spécification : DSP-1.7 (`reload.md`), registre `E-DSP-STATE-TYPE`,
  F-META-035 (`experimental`). Conformance : `types/l5b-dsp3-state-accepted`,
  `types/l5b-dsp3-init-missing` ; `types/dsp-block-state-type` adapté : il
  utilisait `{ gain: f64 }`, que l'ABI 3 ratifiée accepte désormais ; il
  teste maintenant un champ `String` (refus toujours `E-DSP-STATE-TYPE`).
- Mesures : conformance 757 → 755 PASS, 0 FAIL, 2 SKIP ; `dsp_block` (ABI 2)
  4/4 ; `ravum-lang-sema` + `-codegen` : 1 344 tests, 0 échec.

## Pas fait (tranches suivantes)

- **Hôte** (`ravum-patch-host`, `ravum-protocol`) : charger un module ABI 3
  (tampon d'état alloué hors du thread audio avec `size`/`align`, limites
  64 KiB / 64 : DSP3-3), conserver le tampon au même digest (DSP3-4),
  refuser ABI 2 ↔ 3 (DSP3-7), fondu enchaîné (aujourd'hui une copie
  `DspState` sur la pile : à revoir pour un état opaque).
- Migration côté éditeur (DSP3-5), tests DSP3-T1 à T6, inventaire `unsafe`
  des nouveaux sites de l'hôte.
