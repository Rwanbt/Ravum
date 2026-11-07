# Spécification exploratoire — audio temps réel et budget de ressources

## Domaines

- **RT-safe** : interdit sur l'ensemble transitoire du call graph heap alloc/dealloc non bornée, blocage sur lock, IO sync, logs bloquants, thread join, panic non couvert, initialisation lazy dangereuse. Ce profil ne garantit **pas** que le callback respecte son échéance.
- **RT-bounded** : RT-safe, boucles/itérations et consommation de pile bornées, absence de récursion non bornée, buffers préalloués, bornes sur coût logique ; le modèle de coût peut rester abstrait.
- **hard-RT** : RT-bounded + plateforme exacte, flags, CPU, OS RT, interruptions, scheduling, cache/mémoire et WCET évalués ; refus d'un certificat générique pour tout PC.

## Calcul d'échéance

Pour fréquence échantillonnage `Fs` et frames `N`, budget de période `N/Fs` secondes. Exemple `128 / 48000 = 2,666... ms`. L'application ne possède pas nécessairement cette totalité pour un DSP : il faut réserver le reste au pilote, à l'ordonnanceur, aux autres plugins et aux marges de sécurité. Distinguer latence mesurée p99.99 et majoration prouvée.

## Annotations candidates

```rust
@rt_safe(no_heap, no_block, no_panic, no_io)
@budget(max_stack_bytes = 4096, max_iterations = 128)
fn process(input: &[f32; 128], output: &mut [f32; 128], gain: FiniteF32)
    effects(none)
{
    for i in 0..128 { output[i] = input[i] * gain.value(); }
}
```

Syntaxe conceptuelle. Les flottants, denormals, FMA, SIMD, boucle et transfert de `FiniteF32` nécessitent spécifications. L'annotation « no_heap » peut être invalidée par un destructeur/initialisation indirecte : vérifier les appels implicites.

## Hot swapping audio

`compile -> prove -> allocate new graph -> migrate state off-RT -> prepare queue -> nonblocking swap at block boundary -> reclaim old graph off-RT`. Toute attente, lock ou libération imprévisible dans callback est un échec. Une transition sans glitch acoustique ne suit pas automatiquement de l'absence d'xrun ; tester discontinuités et crossfade.

## Matrice essais concrets

1. Tests unitaires DSP bit-exacts en mode strict et tests de tolérance en approximatif.
2. Instrumentation des allocateurs/locks et des chemins de panic (positif et négatif).
3. Bench audio à 32/64/128/256 samples, 44.1/48/96 kHz, mono/stéréo et charges DSP variables.
4. Test E2E réel sur Windows + WASAPI/ASIO si autorisé, Linux JACK/PipeWire, macOS CoreAudio à mesure du portage.
5. Test hot patch d'un filtre IIR avec état d'historique : compatibilité de state, glitch, memory lifetime.
6. Matériel/OS décrits dans tout rapport ; aucun « hard real-time » sans preuve WCET pertinente.

## Sécurité des chemins critiques

- Une fonction externe sans contrat d'effet fiable est considérée **bloquante/allouante** par défaut.
- Une IA n'a pas le droit de lever les contraintes pour améliorer un benchmark.
- Même si le DSP est vérifié, crash matériel/driver/XRUN du système restent possibles.

## Références méthodologiques

Clang Function Effect Analysis : https://clang.llvm.org/docs/FunctionEffectAnalysis.html
PortAudio callback guidance : https://www.portaudio.com/docs/v19-doxydocs/writing_a_callback.html
