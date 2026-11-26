# Première frame de la pile modulaire — r1-pulse, 2026-11-26

Mesures pour la décision UI-D21 (« comprendre et réduire le délai jusqu'à
la première frame »). Aucune recommandation : ce fichier ne contient que des
mesures locales.

## Conditions

- Hôte : R1-Pulse, Ubuntu 26.04.1, noyau 7.0.0-44, session **Wayland**,
  Intel Core i7-13620H, 14 GiB.
- GPU : **Intel Raptor Lake-P UHD Graphics** (Mesa 26.0.8, Vulkan 1.4.335),
  adaptateur Vulkan effectivement utilisé (`libvulkan_intel.so`).
- Écran : dalle interne Sharp LQ156T1JW04, 2560 × 1440 @ 239,97 Hz, échelle
  133 % ; la pile force une surface de **1280 × 800 pixels physiques**.
- Source : branche `bench-u1b-round3-report`, commit **`86dc42e`**,
  `rustc 1.95.0 (59807616e 2026-04-14)`, `cargo build --release --locked -p r3-modular`.
- Alimentation : **secteur branché** (`ADP1 online=1`), batterie 96 %.
  Autres applications fermées, souris non touchée, fenêtre au premier plan.
- Scène : 10 000 lignes, 50 visibles, 5 frames par exécution (`R3_FRAMES=5`),
  instrumentation `R3_PHASES=1` (temps cumulé depuis `main`, écrit sur stderr).
- Commande type :
  `R3_PHASES=1 R3_FRAMES=5 R3_OUT=/tmp/r3.json target/release/r3-modular`
- Toutes les exécutions ont rendu **exit 0** ; aucun message sur stderr hors
  des lignes `phase …`.

## Série 1 — étape 4 : réglage par défaut, cache de pipelines purgé

`rm -rf ~/.cache/ravum-ui-bench`, puis 5 exécutions consécutives, sans rien
entre elles. L'exécution 1 est à froid, les exécutions 2 à 5 réutilisent le
cache.

| Phase (cumul, ms) | Exécution 1 (froide) | Médiane 2–5 (cache chaud) |
|---|---|---|
| boucle d'événements prête | 107,6 | 70,3 |
| fenêtre créée | 128,1 | 85,7 |
| GPU : instance, adaptateur, device, surface | 202,0 | 141,5 |
| pipelines Vello (`Renderer::new`) | 208,2 | 148,3 |
| mise en forme du texte (50 lignes) | 213,0 | 152,2 |
| **première frame présentée** | **260,8** | **199,2** |

Cache après la série : **797 433 octets**, **1 fichier**
(`wgpu_pipeline_cache_vulkan_32902_42920` ; vendor 32902 = 0x8086 Intel,
device 42920 = 0xA7A8).

## Série 2 — étape 5 : ancien réglage (`R3_AA_ALL=1 R3_NO_PIPELINE_CACHE=1`)

5 exécutions, cache de pipelines ni lu ni écrit ; les trois variantes
d'anticrénelage sont compilées.

| Phase (cumul, ms) | Médiane des 5 exécutions |
|---|---|
| boucle d'événements prête | 70,7 |
| fenêtre créée | 88,2 |
| GPU : instance, adaptateur, device, surface | 146,5 |
| pipelines Vello (`Renderer::new`) | 155,7 |
| mise en forme du texte (50 lignes) | 161,6 |
| **première frame présentée** | **209,2** |

La série change **deux** paramètres à la fois (`AA_ALL` et absence de cache) :
l'écart avec la série 1 ne peut pas être attribué à l'un ou à l'autre avec ces
seules données.

## Série 3 — étape 6 : GPU dédié RTX 4070 — **non mesurée**

Les variables demandées (`__NV_PRIME_RENDER_OFFLOAD=1`,
`__GLX_VENDOR_LIBRARY_NAME=nvidia`) **ne redirigent pas Vulkan** : le loader a
chargé le pilote Intel et le rendu est resté sur l'iGPU. Aucun chiffre
« RTX 4070 » n'est donc produit.

Preuves relevées pendant une exécution de 2000 frames avec ces variables :

```text
[Vulkan Loader] DRIVER:  Using "Intel(R) Graphics (RPL-P)" with driver: "/usr/lib/x86_64-linux-gnu/libvulkan_intel.so"
```

```text
# nvidia-smi pmon -c 1
# gpu         pid   type     sm    mem    enc    dec    jpg    ofa    command
    0    1653846     G      0      0      -      -      -      -    r3-modular
```

Interprétation : le processus crée bien un contexte graphique NVIDIA (visible
dans `nvidia-smi`, type `G`, 2 MiB), mais **l'adaptateur Vulkan qui rend la
scène est l'Intel** ; la clé du cache de pipelines (`vendor 32902`,
`device 42920`) le confirme. Les phases de cette exécution sont identiques à
celles de la série 1 à froid (`first_present` 262,2 ms), ce qui est cohérent
avec un rendu sur l'iGPU, pas avec un changement de GPU.

## Synthèse — durées par phase (ms)

Durées calculées par différence des temps cumulés médians ci-dessus (ce ne
sont pas des mesures séparées).

| Phase (durée) | Série 1 froide | Série 1 chaude | Série 2 |
|---|---|---|---|
| démarrage → boucle d'événements prête | 107,6 | 70,3 | 70,7 |
| création de la fenêtre | 20,5 | 15,4 | 17,5 |
| instance / adaptateur / device / surface | 73,9 | 55,8 | 58,3 |
| pipelines Vello | 6,2 | 6,8 | 9,2 |
| mise en forme du texte | 4,8 | 3,9 | 5,9 |
| premier rendu + présentation | 47,8 | 47,1 | 47,6 |
| **jusqu'à la première frame** | **260,8** | **199,2** | **209,2** |

Observations factuelles :

- sur ce pilote, `Renderer::new` (VAO/pipelines Vello) coûte **6–9 ms** ;
  la différence `area_only` ↔ `AA_ALL` est de **+2,4 ms** dans ces séries,
  très loin des ≈ 110 ms mesurées sous lavapipe ;
- le poste le plus lourd hors démarrage initial est le **premier rendu +
  présentation** (≈ 47 ms), identique dans les trois séries ;
- le froid (série 1, exécution 1) ajoute ≈ **62 ms**, concentrées dans
  `gpu_device_surface` (202,0 vs 141,5 ms), pas dans les pipelines Vello ;
- le démarrage jusqu'à la boucle d'événements (70 ms à chaud) est le premier
  poste fixe : il inclut l'instance wgpu, le chargement de la base de polices
  et la création de la boucle.

## Lignes brutes (stderr, format `phase <nom> <ms>`)

### Série 1 (défaut, cache purgé avant l'exécution 1)

```text
[run1] phase event_loop_ready 107.6
[run1] phase window 128.1
[run1] phase gpu_device_surface 202.0
[run1] phase vello_pipelines 208.2
[run1] phase text_layout 213.0
[run1] phase first_present 260.8
[run2] phase event_loop_ready 70.3
[run2] phase window 85.5
[run2] phase gpu_device_surface 145.2
[run2] phase vello_pipelines 151.6
[run2] phase text_layout 155.4
[run2] phase first_present 202.4
[run3] phase event_loop_ready 70.6
[run3] phase window 86.7
[run3] phase gpu_device_surface 142.4
[run3] phase vello_pipelines 148.3
[run3] phase text_layout 152.2
[run3] phase first_present 199.2
[run4] phase event_loop_ready 70.4
[run4] phase window 85.9
[run4] phase gpu_device_surface 140.6
[run4] phase vello_pipelines 148.3
[run4] phase text_layout 152.2
[run4] phase first_present 199.3
[run5] phase event_loop_ready 67.4
[run5] phase window 85.5
[run5] phase gpu_device_surface 139.8
[run5] phase vello_pipelines 145.3
[run5] phase text_layout 149.1
[run5] phase first_present 196.0
```

### Série 2 (`R3_AA_ALL=1 R3_NO_PIPELINE_CACHE=1`)

```text
[run1] phase event_loop_ready 84.7
[run1] phase window 103.0
[run1] phase gpu_device_surface 171.0
[run1] phase vello_pipelines 178.7
[run1] phase text_layout 182.9
[run1] phase first_present 229.0
[run2] phase event_loop_ready 70.7
[run2] phase window 88.2
[run2] phase gpu_device_surface 145.6
[run2] phase vello_pipelines 154.8
[run2] phase text_layout 161.6
[run2] phase first_present 209.2
[run3] phase event_loop_ready 81.7
[run3] phase window 100.7
[run3] phase gpu_device_surface 160.6
[run3] phase vello_pipelines 170.1
[run3] phase text_layout 176.5
[run3] phase first_present 222.9
[run4] phase event_loop_ready 69.4
[run4] phase window 85.6
[run4] phase gpu_device_surface 146.5
[run4] phase vello_pipelines 155.7
[run4] phase text_layout 160.0
[run4] phase first_present 206.8
[run5] phase event_loop_ready 69.1
[run5] phase window 86.0
[run5] phase gpu_device_surface 142.2
[run5] phase vello_pipelines 151.5
[run5] phase text_layout 155.7
[run5] phase first_present 202.1
```

### Série 3 (variables PRIME, 2000 frames) — rendu resté sur l'Intel

```text
phase event_loop_ready 110.5
phase window 127.3
phase gpu_device_surface 204.1
phase vello_pipelines 212.0
phase text_layout 215.9
phase first_present 262.2
```

## Étape 7 — tour complet de la pile modulaire seule

`python3 round3/run_round3.py --stacks modular --runs 5` → **PASS 5/5**,
exit 0, répertoire
[`round3-20261126T163600Z/`](round3-20261126T163600Z/report.json).

| Exécution | 1re frame (ms) | p50 | p95 | p99 | max | CPU/frame (ms) | RSS (Mio) |
|---|---|---|---|---|---|---|---|
| 1 | 259,3 | 4,17 | 4,26 | 4,38 | 34,0 | 1,09 | 190 |
| 2 | 212,3 | 4,17 | 4,27 | 4,63 | 9,5 | 1,02 | 190 |
| 3 | 210,0 | 4,17 | 4,27 | 4,79 | 12,6 | 1,01 | 190 |
| 4 | 212,8 | 4,17 | 4,27 | 4,36 | 11,4 | 1,02 | 188 |
| 5 | 204,9 | 4,17 | 4,27 | 4,34 | 20,3 | 1,01 | 190 |

Médiane des 5 exécutions : **1re frame 212 ms**, p50 4,17 ms, p99 4,38 ms,
CPU 1,02 ms/frame, RSS 190 Mio, binaire 17 602 848 octets, build incrémental
0,3 s. Le rapport du lanceur indique `"dirty": true` parce qu'un répertoire de
résultats non suivi du tour précédent
(`bench/ui-candidates/round3-20261126T154119Z/`) était présent dans le
workspace ; **aucun fichier suivi n'était modifié** (`git status` ne liste que
des `??`).

## Limites

- **Une seule machine**, un seul OS (Wayland), un seul pilote (Mesa/Intel) ;
  pas de Windows, pas de macOS, pas de GPU discret.
- Série 3 **non mesurée** : les variables demandées n'agissent que sur GLX,
  pas sur Vulkan ; aucun chiffre NVIDIA n'est produit.
- « Froid » signifie ici **cache wgpu purgé uniquement** : le cache de shaders
  du pilote (`~/.cache/mesa_shader_cache`, 23 Mo) n'a **pas** été supprimé et
  reste chaud. Le coût du froid réel (machine fraîche, caches pilote vides)
  n'est donc pas mesuré.
- L'exécution froide n'a **qu'un seul échantillon** (n = 1) : pas de médiane,
  pas d'intervalle de confiance.
- Les phases sont des temps **cumulés depuis `main`**, mesurés par
  instrumentation dans le binaire (`R3_PHASES`), écrite sur stderr ; elles
  n'incluent ni le lancement du processus, ni l'édition de liens dynamique
  avant `main`.
- La frontière entre `vello_pipelines` et `first_present` dépend du pilote :
  avec Mesa, la compilation de pipelines peut être différée au premier usage ;
  ces deux postes ne sont donc pas attribuables indépendamment à la
  compilation de shaders.
- 5 frames par exécution : les médianes de phases portent sur le démarrage,
  pas sur le régime établi.
- Gouverneur CPU `powersave`, fréquence non épinglée ; la variabilité entre
  exécutions (ex. série 2, run 1 : 229 ms) n'est pas expliquée par les
  séries ni par les variables contrôlées.
- Pas mesurés : repos, latence d'entrée, VRAM, temps GPU par frame
  (protocole §5) ; texte long, IME, bidi, accessibilité (tour 4).
- Ces chiffres valent pour le commit `86dc42e`, cette machine et cette
  session ; ils ne sont pas `PROVED` et ne décident pas UI-D21 à eux seuls.
