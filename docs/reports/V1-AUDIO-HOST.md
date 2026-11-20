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
### Essai sur périphérique réel (2026-11-20)

MSI Pulse 15 B13VGK (Intel Core i7-13620H, 16 CPU logiques), Ubuntu
26.04.1 LTS x86_64, haut-parleurs intégrés Intel Raptor Lake-P/U/H cAVS
(ALSA `sof-hda-dsp`, PipeWire 1.6.2). Sortie CPAL 0.15.3 en f32,
44 100 Hz, 2 canaux. rustc 1.95.0 (`59807616e`), module
`audio_thread_priority` 0.38.0. Source de départ
`0a43ef42d79e2adc3f03022bd017f94361ceb2ed` avec l’instrumentation de
`play` de ce changement.

`uname -a` indique `7.0.0-43-generic #43-Ubuntu SMP PREEMPT_DYNAMIC`.
La configuration du noyau indique `# CONFIG_PREEMPT_RT is not set`.
RTKit 0.14 actif, `ulimit -r` à 0. `libasound2-dev` 1.2.15.3 installé.
Les fichiers de développement D-Bus 1.16.2 et systemd ont été extraits
dans un répertoire temporaire et exposés via `PKG_CONFIG_PATH`. Le lien
dynamique D-Bus utilise la bibliothèque système. Lecture et charge
exécutées hors du bac à sable pour accéder à la sortie locale.

```bash
cargo test -p ravum-lang-audio --features rt-priority
cargo clippy -p ravum-lang-audio --all-targets --features rt-priority -- -D warnings
cargo run --release -p ravum-lang-audio --features rt-priority --example play
cargo run --release -p ravum-lang-audio --features cpal --example play
ps -T -p "$pid" -o pid,tid,cls,rtprio,comm
stress-ng --cpu 0 --timeout 20s --metrics-brief
```

Quatre tests WAV et Clippy réussis. Pour la charge, les deux binaires
release ont été précompilés avec les mêmes features que ci-dessus, puis
exécutés successivement dans la même fenêtre de 20 s de stress-ng 0.20.01.
Les 16 workers CPU ont tourné avec un temps CPU utilisateur cumulé de
310,14 s. La politique a été relevée environ toutes les 500 ms pendant
la lecture.

| Essai | `rt_status()` | Thread `cpal_alsa_out` | Max `process` (µs) | Blocs au-delà du budget / traités |
|---|---|---|---:|---:|
| Au repos, `rt-priority` | `Promoted` | RR, priorité 10 (TID 1836834) | 24,484 | 0 / 746 |
| Au repos, `cpal` seul | `Disabled` | TS (TID 1836936) | 29,833 | 0 / 762 |
| Sous charge, `rt-priority` | `Promoted` | TS puis RR, priorité 10 (TID 1836535) | 6,726 | 0 / 730 |
| Sous charge, `cpal` seul | `Disabled` | TS (TID 1836598) | 12,009 | 0 / 762 |

La politique observée est `SCHED_RR`. Le thread PipeWire `data-loop.0`
est distinct et était en RR priorité 20 dans les deux configurations.
L’écoute sur place n’a relevé ni craquement ni coupure au changement de
note dans les deux essais sous charge. Les quatre lectures ont observé
les générations 1 puis 2 et compté 0 allocation Rust dans
`RtRunner::process`.

**`TESTED`** pour la promotion du thread CPAL sur cette machine.
L’instrumentation, limitée à l’exemple, compare le temps mural de
`RtRunner::process` à la durée des échantillons du bloc
(`trames / sample_rate`). Aucun bloc n’a été exclu sur les quatre essais.
Elle ne couvre ni l’arrivée tardive du rappel, ni son exécution complète,
ni l’appel initial à RTKit. Le nombre de rappels hors échéance reste
non mesuré. Ces maxima sur une lecture courte ne suffisent pas à comparer
les performances des deux politiques ni à établir une garantie hard-RT.

## Limites

- Mono, `f32` ; pas d'entrée micro (silence) ; pas de choix du périphérique.
- Aucune garantie hard-RT ; sites `unsafe` du JIT (#7b/#8/#9) non audités.
- Le rappel `cpal` imprime une erreur de flux sur `stderr` (hors thread
  audio selon `cpal`, non vérifié ici).
