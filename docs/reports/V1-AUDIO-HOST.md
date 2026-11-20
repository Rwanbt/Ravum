# V1-A1 — Hôte audio : abstraction, hôte WAV, `cpal` optionnel

| Champ | Valeur |
|---|---|
| Décision | V1-A1 (2026-11-20) : abstraction + fichier WAV testable ici + `cpal` derrière une feature désactivée par défaut |
| Crate | `crates/ravum-lang-audio` (`#![forbid(unsafe_code)]`) |
| Source | branche `claude/trusting-cerf-4hhv5a`, parent `6bde4c0` |
| Toolchain | rustc 1.95.0, Linux x86_64 (conteneur, sans périphérique audio) |
| État des preuves | hôte WAV : `TESTED` ; hôte `cpal` : **compilé seulement, `NOT_RUN`** |

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
  du périphérique traité par morceaux, aucune allocation dans le rappel.

## Preuves (`cargo test -p ravum-lang-audio`)

| Test | Résultat |
|---|---|
| `blocks_equal_per_sample_calls_and_swap_between_blocks` | sortie = `x * k` échantillon par échantillon ; v2 publiée après le bloc 3, adoptée exactement au bloc 4 ; bourrage en silence ; **0 allocation** dans `process` (compteur par thread, swap compris) |
| `a_dsp_block_with_state_and_a_trapping_update_keeps_running` | bloc DSP à état ; une v2 qui trappe à chaque échantillon est rattrapée : sortie = filtre v1 de bout en bout |
| `the_wav_file_is_well_formed` | en-tête RIFF/WAVE, écrêtage, taille du fichier |
| `the_allocation_counter_detects_an_allocating_processor` | contrôle **négatif** : le compteur voit une allocation |

`cargo check -p ravum-lang-audio --features cpal` et `cargo clippy` avec la
feature passent (avec `libasound2-dev` installé dans le conteneur) ;
`cargo deny check` : ok. **Aucune exécution sur un périphérique réel.**

## Pour tester sur une machine

```bash
cargo test -p ravum-lang-audio --features cpal
```

puis, dans un programme : `HotEngine::start(&ir, "gain")` et
`CpalHost::start(Box::new(runner))` ; garder `CpalHost` vivant ; publier
les éditions avec `engine.update(..)` depuis un autre thread.

## Limites

- Mono, `f32` ; pas d'entrée micro (silence) ; pas de choix du périphérique.
- Aucune garantie hard-RT ; sites `unsafe` du JIT (#7b/#8/#9) non audités.
- Le rappel `cpal` imprime une erreur de flux sur `stderr` (hors thread
  audio selon `cpal`, non vérifié ici).
