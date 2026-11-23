# V2-L4-dsp-audio — Gain et filtre IIR dans l’hôte WAV

| Champ | Valeur |
|---|---|
| Gates | T8 et T9 |
| Branche / SHA de départ / SHA final | `v2/L4-dsp-audio` / `0aa236474d3a141cdcf72d54b4352245a81dee37` / commit qui ajoute ce rapport |
| SHA du code mesuré | `5229d7daf8e614ec20f0353ed347277db5eb6a28` |
| Outils et plateforme | Cargo 1.95.0, Cranelift 0.116.1, Linux 7.0.0-43-generic x86_64, Intel Core i7-13620H |
| rustc | `1.95.0 (59807616e 2026-04-14)` |

## Objectif et périmètre

Exécuter un DSP Ravum dans l’hôte WAV avec le JIT et mesurer deux bascules
à chaud. P0-C, P0-D, P0-F et INV-05 s’appliquent. Le test reprend l’état de
DSP-1.2 et la bascule entre blocs documentée dans T16.

## Modification (fichiers)

- `examples/dsp-gain-iir/audio.rv` ajoute un trajet mono `f32`, gain suivi
  d’un filtre IIR d’ordre 1. L’état reste `{ z1, z2 }`. Le filtre utilise
  `z1` et initialise `z2` à zéro. L’exemple hors ligne `f64` est conservé.
- `crates/ravum-lang-audio/tests/wav.rs` charge le DSP dans le JIT, réutilise
  `WavHost` et publie les éditions via le rappel entre blocs.

## Tests positifs / négatifs

Le nouveau test `gain_iir_wav_keeps_state_across_gain_and_coefficient_edits`
compare les 192 échantillons bit à bit à la référence Rust. Il vérifie
que chaque édition ne recompile que sa fonction cible. L’égalité à chaque
échantillon vérifie la conservation de l’état et l’instant d’adoption.

La voie Ravum → Rust → rustc produit les mêmes 192 échantillons. Le paquet
temporaire Core 1.0 reprend `audio.rv` et ajoute deux copies suffixées des
fonctions `gain`, `coefficient` et `process_sample` avec les coefficients
ci-dessous. Le `main` appelle successivement les trois versions avec le même
état et imprime chaque résultat. Les valeurs lues sont comparées en bits
`f32` à la même récurrence que celle du test WAV.

Les trois versions compilées en bibliothèques avec `--emit dsp-block`
produisent également 192 échantillons identiques bit à bit. L’essai appelle
`ravum_process_block` via `ctypes` sur trois tampons de 64 trames, en
réutilisant le même état C à deux champs. `z2` reste égal à zéro.

Les tests existants vérifient également le retour à la version précédente
après un trap et la détection d’une allocation injectée. Les cinq tests WAV
réussissent.

## Commandes exécutées

```sh
cargo build -p ravum-lang-cli --bin ravum-lang
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p ravum-lang-audio -- --nocapture
target/debug/ravum-lang run "$ravum_l4_rustc_package"
target/debug/ravum-lang check "$ravum_l4_dsp_block" --dsp-block
target/debug/ravum-lang build "$ravum_l4_dsp_block" --emit dsp-block
cargo run -p ravum-conformance --release -- run
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo --config 'profile.test.package.sha2.opt-level=3' \
  test --workspace --no-fail-fast -- --test-threads=1
```

## Résultats mesurés

Entrée constante à 0,25, 192 trames mono à 48 kHz, blocs de 64 trames.
Pour une bascule d’indice `n`, le saut est
`abs(sortie[n] - sortie[n - 1])` avant la quantification PCM.

| Bascule | Saut d’amplitude normalisée | Longueur du fondu |
|---|---:|---:|
| Gain de 2 à 1, trame 64 | 0,049999923 | 0 trame |
| Coefficient de 0,8 à 0,6, trame 128 | 0,050000086 | 0 trame |
| Maximum | 0,050000086 | 0 trame |

Le lecteur bascule directement et le compteur relève **0 allocation Rust**
dans les trois appels à `process`, adoption comprise.

## Limites, écarts, demandes hors périmètre

L’essai couvre le rendu WAV sur ce signal. La perception du raccord, CPAL,
les allocations natives et la durée du rappel sont `NOT_RUN`. Le workspace
est vérifié sur système de fichiers Linux natif, avec le correctif de masquage
de conformance `64efd691b2b18fd2e5cb5e86fed98a63ab0e8891` appliqué dans la
copie de test. L’intégration de ce correctif relève de l’intégrateur.
La première exécution a rencontré le quota du stockage temporaire. La relance
réutilise le même checkout après nettoyage des anciens artefacts de tests.

## État des preuves

`TESTED` pour le trajet DSP WAV, les deux bascules, la conservation de
l’état, l’accord avec rustc et le comptage des allocations Rust.
Conformance `TESTED` avec 743 réussites, 0 échec et 2 `SKIP` sous Linux.
La dimension `--verify-build` est `NOT_RUN`.
Workspace `TESTED` avec 2 321 réussites, 0 échec et 14 ignorés, dans le
checkout et le profil décrits ci-dessus. Formatage et Clippy workspace
réussissent. L’exécution par la CI distante est `NOT_RUN`.

## Proposition à l’intégrateur

Ajouter ces résultats comme preuves L4 pour T8 et T9. La mesure caractérise
la bascule directe de l’hôte WAV et laisse la qualification globale des
deux gates à l’intégrateur.
