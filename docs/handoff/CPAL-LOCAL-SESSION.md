# Consigne — session locale : exécuter l'hôte audio `cpal` (V1-A2)

À copier-coller tel quel dans une session Claude Code **locale** (sur une
machine avec une sortie audio), à la racine d'un clone de `rwanbt/ravum`.

---

Tu reprends une tâche du plan V1 de Ravum (décisions V1-A1/V1-A2,
`docs/decision/DECISIONS-2026-11-19-V1.md`). Lis d'abord `AGENTS.md`, puis
`docs/reports/V1-AUDIO-HOST.md` et `crates/ravum-lang-audio/` (src/lib.rs,
src/device.rs, tests/wav.rs). Réponds en français.

Contexte : le crate `ravum-lang-audio` fournit un trait `Processor`
(implémenté par `ravum_lang_jit::hot::RtRunner`), un hôte WAV testé en CI, et
un hôte `CpalHost` derrière la feature `cpal` (désactivée par défaut) qui n'a
**jamais été exécuté** (le conteneur cloud n'a pas de périphérique) : état
`NOT_RUN`. Ton travail : le faire tourner ici, sur un vrai périphérique, et
rapporter honnêtement.

1. `git fetch origin claude/trusting-cerf-4hhv5a && git checkout claude/trusting-cerf-4hhv5a`
   (travaille sur cette branche ; pas de force push, pas de PR, pas de merge).
2. Note l'OS, le matériel audio, la version de rustc et le SHA (`git rev-parse HEAD`).
3. `cargo build -p ravum-lang-audio --features cpal` puis
   `cargo test -p ravum-lang-audio --features cpal` (sous Linux il faut
   `libasound2-dev` ; sous Windows/macOS rien de plus).
4. Écris un exemple `crates/ravum-lang-audio/examples/play.rs` (feature
   `cpal`, `required-features = ["cpal"]` dans Cargo.toml) qui :
   - compile un `.rv` `fn gain(x: f32) -> f32` — ou mieux un oscillateur via
     un bloc DSP `process_sample(state: &mut State, input: f32) -> f32` avec
     état (phase), car l'entrée de `CpalHost` est du silence ;
   - démarre `HotEngine::start`, puis `CpalHost::start(Box::new(runner))` ;
   - joue ~2 s, publie une **édition à chaud** (`engine.update`, ex. changer
     la fréquence ou le gain) depuis le thread principal, joue encore ~2 s ;
   - affiche `sample_rate`, `channels`, et les générations vues.
   Lance-le : `cargo run -p ravum-lang-audio --features cpal --example play`.
5. Vérifie et rapporte : le son est-il audible, sans craquements ? le
   changement à chaud est-il entendu sans coupure ? le format du
   périphérique est-il `f32` (sinon `CpalHost` refuse : note-le, et propose
   — sans l'imposer — un support i16/u16) ? aucune allocation dans le rappel
   (si possible : compteur d'allocations comme dans `tests/wav.rs`, armé
   uniquement dans le rappel audio).
6. Mets à jour `docs/reports/V1-AUDIO-HOST.md` (section « Exécution sur
   périphérique » : machine, OS, commandes, résultats, limites) et la ligne
   T16 de `docs/roadmap/CORE-1.0-GATE-MATRIX.md` (`cpal` : `NOT_RUN` →
   `TESTED` seulement si réellement exécuté et entendu ; sinon dire
   précisément ce qui a échoué). Aucune garantie hard-RT n'est établie.
7. Règles : ne désactive ni ne supprime aucun test, n'affaiblis aucun
   seuil ; `SKIP`/`NOT_RUN` ne sont jamais `PASS` ; pas d'identifiant de
   modèle dans les commits ; commit + push sur
   `claude/trusting-cerf-4hhv5a` à la fin.
