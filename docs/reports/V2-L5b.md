# V2-L5b — T8 : ABI DSP 3 (RFC-DSP-ABI-2), tranches 1 à 3

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

## Tranche 2 : hôte (fait)

- `ravum-protocol` : `DSP3_ABI_VERSION`, `DSP3_SIGNATURE`, limites
  `DSP3_MAX_STATE_BYTES` (64 KiB) / `DSP3_MAX_STATE_ALIGN` (64),
  `RejectReason::StateLayout` (ajout).
- `ravum-patch-host::block3` : `prepare_block3` (ABI 3 vérifiée, schéma et
  layout lus, layout hors bornes refusé), `Block3Host::new` (tampon alloué et
  initialisé par le module, hors thread audio), `process` (sans allocation ni
  verrou), `replace` (même digest et même layout : état conservé ; sinon
  `StateSchemaChanged`, l'ancien bloc continue ; même artefact :
  `AlreadyActive` ; l'ancien module est rendu à l'appelant, détruit hors
  thread audio).
- Tests `crates/ravum-lang-cli/tests/dsp3_block.rs` (3) : bloc `{ y }` =
  référence Rust bit à bit sur 4 096 échantillons avec remplacement à chaud
  au même schéma et état conservé (DSP3-T1/T2/T4) ; état à struct imbriquée,
  tableau et `u32` : layout 20/4 et sortie de la ligne à retard ; nouveau
  schéma refusé et bloc courant gardé ; ABI 2 refusé par `prepare_block3`
  et ABI 3 refusé par `prepare_block` (DSP3-7, DSP3-T5). ABI 2 inchangée :
  `dsp_block` 4/4 (DSP3-T6). `ravum-patch-host` + `ravum-protocol` : 38/0.
- `unsafe` : sites inscrits dans `docs/ffi/SAFETY.md` §9 et au dossier
  d'audit ; `tools/unsafe_audit.py` : PASS.

## Tranche 3 : fondu enchaîné, allocation, mesure (fait)

- `Block3Host::replace_with_fade(module, frames)` : la copie de l'état pour
  l'ancien module est allouée et remplie sur le thread appelant ; pendant le
  fondu, l'ancien module tourne sur sa copie, le nouveau sur l'état de
  l'hôte, sortie `ancien·(1−w) + nouveau·w`, `w = k/frames` ; un fondu fini
  est seulement marqué, libéré par `collect()` sur le thread appelant.
- Test `a_crossfaded_swap_mixes_both_versions_and_allocates_nothing_in_process` :
  sortie = formule de référence **bit à bit** (4 096 échantillons, fondu de
  256 trames) ; **0 allocation et 0 libération** dans `process`
  (allocateur compteur par thread), fondu compris.
- **Mesure du fondu sur la voie `cdylib`** (`dsp-gain-iir`, gain 2 → 1,
  sinus, ce commit, Linux, debug) : au changement |Δy| = 0,036418 avec fondu
  (0,017714 sans), |Δ²y| = 0,000203 (0,000077 sans) ; |Δ²y| maximal du
  signal stable : 0,001351. **Aucun clic dans les deux cas** : le filtre IIR
  lisse déjà le changement de gain, le fondu n'apporte rien de mesurable
  ici. La première différence mesure surtout la pente du signal (gain 2
  encore dominant au début du fondu). Aucune amélioration n'est revendiquée.

## Pas fait

- Publication sans verrou (comme `BlockHost` pour l'ABI 2) : `Block3Host`
  est possédé par un thread et remplacé par `&mut self`.
- ~~Migration (DSP3-5)~~ : **faite**. Source : `fn migrate(old: OldState)
  -> State` (vérifiée : `OldState` type d'état ABI 3 ≠ `State`, sinon
  `E-DSP-ENTRY`, cas `types/l5b-dsp3-migrate-invalid`). Générateur :
  `ravum_state_migrate_from` (digest de `OldState`), `ravum_state_migrate`.
  Hôte : `replace` accepte un autre schéma seulement si le module migre
  depuis le schéma courant ; nouveau tampon alloué et rempli sur le thread
  appelant, ancien libéré là ; le fondu copie l'état **avant** la migration.
  Test `a_new_schema_is_accepted_through_migrate_and_refused_without_it` :
  `{ y }` → `{ y, n }`, sortie = référence bit à bit (y conservé) ; module
  migrant depuis un autre schéma refusé, bloc courant gardé. **Limite** : une
  panique dans `migrate` traverse `extern "C"` et arrête le processus (la RFC
  prévoyait un refus) ; non rattrapée.
- ~~Scénario avec un vrai saut~~ : **fait**,
  `the_crossfade_removes_the_step_of_a_pure_gain_edit` — gain pur 1,0 → 0,2
  sur une entrée constante 0,5 : saut de **0,400000** sans fondu, pas
  maximal de **0,001563** avec un fondu de 256 trames (≈ 0,4 / 256) ; signal
  identique après le fondu. Mesure liée à ce signal et à cette édition.
