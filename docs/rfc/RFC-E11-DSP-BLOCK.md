# RFC-E11-DSP-BLOCK — Bloc DSP `.rv` dans l'hôte de patch (réouverture de RFC-RV-DSP-BLOCK-1)

| Champ | Valeur |
|---|---|
| Identifiant | `RFC-E11-DSP-BLOCK` |
| Statut | **`PROPOSED`** — aucune implémentation avant décision du responsable sur §6 |
| Date | 2026-11-19 |
| Auteur | agent de développement |
| Origine | [D-18-2](../decision/DECISIONS-2026-11-18.md) annule A2 ; ferme T8 (avec RFC-E11-HOT-RELOAD) et la part DSP de T9 |
| Reprend | [`RFC-RV-DSP-BLOCK`](RFC-RV-DSP-BLOCK.md) (`DEFERRED` par A2) : son §1 (ABI lue dans le code) reste la référence |
| Portée | `crates/ravum-lang-codegen` (wrapper), `crates/ravum-lang-cli` (`--emit dsp-block`), `crates/ravum-protocol/src/dsp.rs`, `crates/ravum-patch-host`, `examples/dsp-gain-iir` |

> **Ce que cette RFC ne promet pas.** Ni « temps réel garanti », ni hard-RT, ni
> WCET. Elle relie le compilateur à un hôte qui a **mesuré** 0 allocation au
> callback sur des tests ; elle ne qualifie pas une plateforme audio.

## 1. Ce qui change par rapport à RFC-RV-DSP-BLOCK-1

RFC-RV-DSP-BLOCK-1 attendait trois ratifications : l'ABI, le digest de schéma
d'état, le confinement de l'`unsafe` généré. Cette RFC les reprend **telles
quelles** et ajoute :

1. le digest de schéma est **celui de RFC-E11-HOT-RELOAD HR-1.1**, pas un
   format propre au DSP ;
2. le corps du bloc est vérifié sous le profil **`rt`** existant (RFC-RT-TRAPS,
   `ACCEPTED`) : pas d'allocation, pas de trap implicite ;
3. l'état DSP n'est plus le `DspState { z1, z2 }` fixe : c'est une struct `.rv`
   de champs `f32`/`f64`/`i32`/tableaux de taille fixe, possédée par l'hôte.

## 2. Surface source

```rust
struct Biquad { z1: f32, z2: f32 }

fn init() -> Biquad effects() { Biquad { z1: 0.0, z2: 0.0 } }

fn process(state: &mut Biquad, input: &[f32; 64], output: &mut [f32; 64]) -> () effects() {
    // profil rt : aucune allocation, opérations wrapping/saturating explicites
}
```

`ravum-lang build --emit dsp-block --profile rt` produit un `cdylib` qui
exporte `ravum_process_block` (ABI de `ravum-protocol/src/dsp.rs`) et
`ravum_state_schema` (digest HR-1.1).

| ID | Règle proposée | Niveau |
|---|---|---|
| DSP-1.1 | `--emit dsp-block` exige `--profile rt` ; sinon exit 2. | MUST |
| DSP-1.2 | L'état ne contient que des scalaires `f32`/`f64`/`i32`/`u32`/`bool` et des tableaux de taille fixe de ceux-ci (`E-DSP-STATE-TYPE`). | MUST |
| DSP-1.3 | L'`unsafe` du wrapper généré est confiné à un module unique, listé dans `SAFETY.md`, jamais écrit par l'utilisateur. | MUST |
| DSP-1.4 | L'hôte refuse un bloc dont le digest diffère de l'état courant (`RejectReason::StateSchemaChanged`), comme HR-1.3. | MUST |
| DSP-1.5 | Taille de bloc fixée à la compilation (64 par défaut) ; l'hôte refuse une taille différente. | MUST |

## 3. Qualification

- Différentiel : la sortie du bloc `.rv` égale bit à bit celle du filtre de
  référence Rust de `ravum-dsp-harness` sur 10 signaux fixés.
- Compteur d'allocations (allocateur instrumenté existant) : **0** sur le
  callback pendant 10 000 blocs et 20 remplacements.
- Remplacement compatible : état continu (pas de discontinuité au-delà du fondu
  `MAX_FADE_FRAMES = 512`).
- Remplacement incompatible : refusé, l'ancien bloc continue.
- Trois OS en CI (`dsp stress` existe ; il a des délais locaux connus, sans lien
  avec ce travail, à surveiller).

## 4. Vertical T9

`examples/dsp-gain-iir` existe en `.rv` (exécutable) : il devient un bloc
chargé par l'hôte, avec un test de bout en bout qui modifie le gain pendant
l'exécution.

## 5. Dépendances

- RFC-E11-HOT-RELOAD pour HR-1.1 (digest) ;
- aucune dépendance sur NET, SQLITE ni CONCURRENCY (l'hôte gère son thread audio).

## 6. Décisions attendues du responsable

1. Ratifier l'ABI de `RFC-RV-DSP-BLOCK` §1 (inchangée).
2. Accepter le digest commun avec le hot reload.
3. Accepter le confinement DSP-1.3 dans `SAFETY.md`.
4. Accepter les critères de §3 comme définition de la part DSP de T8/T9.
