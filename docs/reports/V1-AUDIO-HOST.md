# V1-A1 — Hôte audio : abstraction, hôte WAV, `cpal` optionnel

| Champ | Valeur |
|---|---|
| Décision | V1-A1 (2026-11-20) : abstraction + fichier WAV testable ici + `cpal` derrière une feature désactivée par défaut |
| Crate | `crates/ravum-lang-audio` (`#![forbid(unsafe_code)]`) |
| Source | branche `claude/trusting-cerf-4hhv5a`, parent `6bde4c0` |
| Toolchain | rustc 1.95.0, Linux x86_64 (conteneur, sans périphérique audio) |
| État des preuves | hôte WAV : `TESTED` ; hôte `cpal` : **`TESTED` sur une sortie réelle** (V1-A2, essai ci-dessous) |

## Interface

- `trait Processor: Send { fn process(&mut self, input: &[f32], output: &mut [f32]) }`
  — mono, appelé sur le thread audio.
- `impl Processor for RtRunner` : `poll()` une fois par bloc (point sûr :
  une version publiée par l'éditeur est adoptée au début du bloc suivant),
  puis l'entrée compilée par échantillon (`process_sample`, `fn(f32)`,
  `fn(f64)`) ; une entrée sans argument produit du silence (elle n'est
  jamais exécutée sur le thread audio). Ajout côté JIT : `RtRunner::kind()`.
- `WavHost` : rendu hors ligne bloc par bloc (tampons alloués avant le
  traitement), rappel `between(k)` côté hôte entre deux blocs (lieu d'une
  mise à jour éditeur), écriture WAV PCM 16 bits mono.
- `device::CpalHost` (feature `cpal`) : sortie par défaut, format `f32`
  seulement (sinon refus explicite), entrée silencieuse, sortie copiée sur
  tous les canaux ; tampon de travail fixe de 256 trames capturé par le rappel (alloué une fois à la création), bloc
  du périphérique traité par morceaux, aucune allocation explicite pour ces tampons pendant le rappel.

## Preuves (`cargo test -p ravum-lang-audio`)

| Test | Résultat |
|---|---|
| `blocks_equal_per_sample_calls_and_swap_between_blocks` | sortie = `x * k` échantillon par échantillon ; v2 publiée après le bloc 3, adoptée exactement au bloc 4 ; bourrage en silence ; **0 allocation** dans `process` (compteur par thread, swap compris) |
| `a_dsp_block_with_state_and_a_trapping_update_keeps_running` | bloc DSP à état ; une v2 qui trappe à chaque échantillon est rattrapée : sortie = filtre v1 de bout en bout |
| `the_wav_file_is_well_formed` | en-tête RIFF/WAVE, écrêtage, taille du fichier |
| `the_allocation_counter_detects_an_allocating_processor` | contrôle **négatif** : le compteur voit une allocation |

`cargo check -p ravum-lang-audio --features cpal` et `cargo clippy` avec la
feature passent (avec `libasound2-dev` installé dans le conteneur) ;
`cargo deny check` : ok. L’essai sur périphérique réel est consigné ci-dessous.

## Exécution sur périphérique

- 2026-11-20, MSI Pulse 15 B13VGK, Ubuntu 26.04.1 LTS x86_64,
  noyau `7.0.0-43-generic`. Sortie par défaut de la session PipeWire 1.6.2 :
  haut-parleurs intégrés Raptor Lake-P/U/H cAVS (`sof-hda-dsp`).
  Exécution hors du bac à sable, qui masque `/dev/snd`.
- Source de départ `96272bc4f84e9862e4adf50b901e7dedfd087e38` (branche
  `claude/trusting-cerf-4hhv5a`) avec l’exemple `play` de ce changement.
  rustc 1.95.0 (`59807616e`), CPAL 0.15.3, ALSA 1.2.15.3
  (`pkg-config --modversion alsa`), profil Cargo de développement.

```bash
cargo build -p ravum-lang-audio --features cpal
cargo test -p ravum-lang-audio --features cpal
cargo clippy -p ravum-lang-audio --features cpal --all-targets -- -D warnings
cargo run -p ravum-lang-audio --features cpal --example play
```

- Build et Clippy réussis. Quatre tests WAV réussis, dont le retour à la
  version précédente sur trap et le contrôle négatif du compteur.
- CPAL a ouvert le flux par défaut en **f32, 44 100 Hz, 2 canaux**.
  Génération 1 observée à 370 blocs traités, puis génération 2 publiée et
  observée à 746 blocs cumulés. L’édition double le pas de phase du
  triangle (441 → 882 Hz à ce taux) en conservant son état.
- Écoute sur place confirmée par le responsable : deux notes audibles,
  passage à l’octave sans craquement ni coupure perceptibles pendant cet
  essai. Statut CPAL **`TESTED`** pour cette configuration.
- **0 allocation Rust** comptée sur les 746 blocs. Le compteur est armé
  dans le rappel uniquement autour de `RtRunner::process`, adoption
  comprise. Il ne mesure ni le reste du rappel CPAL, ni les allocations
  natives de CPAL/ALSA/PipeWire, ni les libérations. Le format annoncé est
  celui du flux CPAL. Le format matériel sous-jacent et le chemin de
  refus d’un flux non-f32 n’ont pas été mesurés lors de cet essai.

## Priorité temps réel (feature `rt-priority`, V1-A3)

- `cargo build -p ravum-lang-audio --features rt-priority` (Linux : il faut
  `libdbus-1-dev` pour RTKit). La feature implique `cpal`.
- Au **premier** rappel, avant tout bloc, le thread du backend est promu par
  `audio_thread_priority::promote_current_thread_to_real_time(frames,
  sample_rate)` ; la poignée vit aussi longtemps que le flux. Cet appel peut
  allouer et bloquer (D-Bus) : exception **unique et documentée** à
  l'INV-05, avant toute production de son. Les rappels suivants n'en font
  rien.
- `CpalHost::rt_status()` dit ce qui s'est passé ; l'exemple `play`
  l'affiche. Un refus (`Refused`) laisse le son à priorité normale.
- État : compilé, clippy et tests du crate verts dans le conteneur cloud ;
  **`NOT_RUN`** sur périphérique. Même promu, aucune garantie hard-RT : un
  noyau non temps réel peut encore préempter.

## Limites

- Mono, `f32` ; pas d'entrée micro (silence) ; pas de choix du périphérique.
- Aucune garantie hard-RT ; sites `unsafe` du JIT (#7b/#8/#9) non audités.
- Le rappel `cpal` imprime une erreur de flux sur `stderr` (hors thread
  audio selon `cpal`, non vérifié ici).
