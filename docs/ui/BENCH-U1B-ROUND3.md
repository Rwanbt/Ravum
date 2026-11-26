# U1-B — Tour 3 : résultats (r1-pulse)

Statut : `TESTED` sur **une** machine, commit `fcb53bb`. Aucune pile n'est
adoptée ; le choix reste au responsable (UI-D4). Protocole :
[`BENCH-U1B-ROUND3-PROTOCOL.md`](BENCH-U1B-ROUND3-PROTOCOL.md). Données brutes :
[`results/r1-pulse/round3-20261126T154119Z/`](../../bench/ui-candidates/results/r1-pulse/round3-20261126T154119Z/report.json).

## Machine
Linux 7.0 x86_64, Wayland ; rendu sur l'iGPU Intel Raptor Lake-P UHD
(Mesa 26.0.8, Vulkan 1.4) — la RTX 4070 Laptop n'est **pas** utilisée ;
écran 2560 × 1440 à 240 Hz ; secteur. 5 exécutions × 600 frames, 10 000 lignes.

## Résultats (médiane des 5 exécutions ; max = pire des 5)

| Pile | Verdict | 1re frame (ms) | p50 | p95 | p99 | max (ms) | CPU/frame (ms) | RSS (Mio) | Binaire (Mio) |
|---|---|---|---|---|---|---|---|---|---|
| egui | PASS | 171 | 4,163 | 4,297 | 4,492 | 13,6 | 0,96 | 185 | 22,3 |
| Slint (réf. seulement, UI-D12) | PASS | 87 | 4,161 | 4,443 | 5,725 | 18,7 | 2,18 | 133 | 28,4 |
| Modulaire (winit, wgpu, Vello, Parley) | PASS | 215 | 4,166 | 4,276 | **4,352** | 35,3 | 1,05 | 191 | **16,8** |

## Lecture
- Les trois piles tiennent le 240 Hz : p50 ≈ 4,16 ms = une période
  d'écran. Le banc est **borné par la synchronisation verticale** ; il ne
  départage pas le débit maximal.
- Régularité : la modulaire a le meilleur p95 et p99 ; Slint le moins bon.
- CPU par frame : egui et modulaire à égalité (≈ 1 ms) ; Slint ≈ 2,2 ms.
- Pic isolé de la modulaire : 35 ms et première frame à 293 ms sur la
  **1re** exécution seulement (22 ms puis ≤ 15 ms ensuite) — cohérent avec
  un démarrage à froid (caches de shaders et de polices) ; à confirmer.
- Démarrage : Slint le plus rapide (87 ms) ; modulaire la plus lente
  (≈ 215 ms) ; mémoire : Slint la plus sobre, egui et modulaire proches.
- Binaire : la modulaire est la plus petite.

## Limites
- Une seule machine, un seul OS, un iGPU ; pas de Windows ni de macOS.
- Pas mesurés : repos (CPU et réveils), latence d'entrée, VRAM, temps GPU
  (protocole §5) ; texte long, IME, bidi, accessibilité (tour 4).
- La pile modulaire du banc est un runtime minimal écrit pour le banc.
