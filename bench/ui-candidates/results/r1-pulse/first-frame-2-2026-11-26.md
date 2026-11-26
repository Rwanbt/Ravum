# Démarrage parallèle de la pile modulaire — r1-pulse, 2026-11-26 (passe 2)

Deuxième étape d'UI-D21 : la préparation (instance, adaptateur, device wgpu,
pipelines Vello, rendu de préchauffage, base de polices) a été déplacée dans
un thread parallèle à la boucle d'événements et à la fenêtre. Ce fichier ne
contient que des mesures ; **aucune recommandation**.

## Conditions

- Hôte : R1-Pulse, Ubuntu 26.04.1, noyau 7.0.0-44, session **Wayland**,
  i7-13620H, 14 GiB.
- GPU : **Intel Raptor Lake-P UHD** (Mesa 26.0.8, Vulkan 1.4.335) ; la
  RTX 4070 n'a pas pu être sélectionnée (voir « Série GPU »).
- Écran : Sharp LQ156T1JW04, 2560 × 1440 @ 239,97 Hz, échelle 133 % ; la
  pile force une surface de **1280 × 800 pixels physiques**.
- Source : branche `bench-u1b-round3-report`, commit **`2495f98`**,
  `rustc 1.95.0 (59807616e 2026-04-14)`,
  `cargo build --release --locked -p r3-modular`.
- Alimentation : **secteur branché** (`ADP1 online=1`), batterie 96 % ;
  autres applications fermées, souris non touchée, fenêtre au premier plan.
- Scène : 10 000 lignes, 50 visibles, 5 frames par exécution
  (`R3_FRAMES=5`), phase écrite sur stderr (`R3_PHASES=1`), temps cumulé
  depuis `main`.
- Commande de base :
  `R3_PHASES=1 R3_FRAMES=5 R3_OUT=/tmp/r3.json target/release/r3-modular`
- 6 exécutions par série, **24/24 en exit 0**, aucune sortie stderr hors
  lignes `phase`.

### Séries et ordre d'exécution

| Série | Variables | Ce qui est mesuré |
|---|---|---|
| **A** | `R3_SERIAL_INIT=1 R3_NO_PREWARM=1` | référence : même travail, sur le thread principal avant la boucle |
| **B** | `R3_NO_PREWARM=1` | préparation parallèle, sans rendu de préchauffage |
| **C** | *(aucune)* | parallèle + préchauffage hors écran 64 × 64 |
| **D** | *(aucune)*, caches vidés avant **chaque** exécution | comme C, à froid réel (caches wgpu **et** Mesa supprimés) |

A, B et C ont été **entrelacées** (A1, B1, C1, A2, B2, C2, …) pour limiter la
dérive thermique. D forme un bloc séparé de 6 exécutions consécutives : vider
les caches avant chaque exécution de D aurait sinon contaminé la chaleur des
séries A/B/C suivantes.

Nuance sur A : même en mode « série », la base de polices garde son thread
propre (`w_fonts` s'exécute pendant l'instance et le device) ; A sérialise le
GPU et les pipelines par rapport à la boucle et à la fenêtre, pas les polices.

## Médianes par phase (temps cumulé depuis `main`, ms)

| Phase (cumul, ms) | A série | B parallèle | C + préchauffage | D froid réel |
|---|---|---|---|---|
| polices (`w_fonts`) | 18,8 | 19,9 | 20,2 | 23,4 |
| instance wgpu (`w_instance`) | 59,5 | 56,4 | 57,9 | 57,5 |
| device (`w_device`) | 110,3 | 108,8 | 110,6 | 123,0 |
| pipelines Vello (`w_pipelines`) | 116,2 | 114,3 | 118,1 | 532,6 |
| rendu de préchauffage (`w_prewarm`) | — | — | 168,9 | 594,4 |
| `main` prêt (`main_ready`) | 116,3 | 0,1 | 0,1 | 0,1 |
| boucle d'événements prête | 118,1 | 2,2 | 2,1 | 2,2 |
| fenêtre | 132,6 | 17,0 | 17,5 | 17,0 |
| jointure du thread (`warm_joined`) | 132,7 | 124,2 | 178,6 | 603,9 |
| surface | 136,2 | 126,1 | 180,8 | 607,8 |
| `vello_pipelines` (reconstruction éventuelle) | 136,2 | 126,2 | 180,8 | 607,8 |
| mise en forme du texte | 139,2 | 129,0 | 183,7 | 610,6 |
| **première frame présentée** | 184,2 | 172,2 | 188,3 | 618,6 |

## Statistiques de la première frame

| Série | Médiane | Min | Max |
|---|---|---|---|
| A | 184,2 | 176,5 | 227,2 |
| B | 172,2 | 164,3 | 185,3 |
| C | 188,3 | 184,7 | 194,0 |
| D | 618,6 | 604,5 | 629,3 |

Écarts entre médianes : **B − A = −12,0 ms** ; **C − B = +16,1 ms** ;
**D − C = +430,3 ms**. Les plages de A et B se recouvrent partiellement et
n = 6 par série : ces écarts n'ont fait l'objet d'aucun test statistique.

Faits relevés :

- en A, `main_ready` à 116,3 ms signifie que la boucle d'événements ne
  démarre qu'après la préparation ; en B/C/D `main_ready` est à 0,1 ms et la
  fenêtre est prête à ≈ 17 ms, pendant que le thread de préparation travaille ;
- à chaud, `w_pipelines` (création des pipelines Vello) vaut 114–118 ms de
  temps cumulé, soit **≈ 5–8 ms de durée** ; à froid réel (D) il monte à
  **532,6 ms** de cumul, soit ≈ 410 ms de durée : la traduction naga est identique
  dans les quatre séries, donc ce surcoût vient de la compilation de shaders par
  le pilote, les deux caches disque (wgpu et Mesa) ayant été vidés ;
- `w_prewarm` coûte ≈ 50 ms au thread (C : 118,1 → 168,9) et retarde la
  jointure (`warm_joined` 178,6 vs 124,2 en B) ; la première frame de C est
  **16,1 ms plus tardive** que celle de B dans ces mesures ;
- en B, le coût de compilation différée réapparaît après `text_layout`
  (129,0 → 172,2, soit ≈ 43 ms) ; en C il est déplacé dans le thread
  (183,7 → 188,3, soit ≈ 5 ms) ;
- à froid réel (D), la première frame est à 618,6 ms ; l'essentiel du surcoût
  est dans la création des pipelines (532,6 ms de cumul à `w_pipelines`, contre
  118,1 ms à chaud).

## Série GPU (étape 4) — **non mesurée**

`MESA_VK_DEVICE_SELECT=10de:*` **ne sélectionne pas** la RTX 4070 : le pilote
chargé pour le rendu reste l'Intel. Une exécution d'identification avec
`VK_LOADER_DEBUG=driver` (non comptée dans les séries) donne :

```text
[Vulkan Loader] DRIVER:                Using "Intel(R) Graphics (RPL-P)" with driver: "/usr/lib/x86_64-linux-gnu/libvulkan_intel.so"
```

Le cache de pipelines écrit par cette exécution porte la clé
`wgpu_pipeline_cache_vulkan_32902_42920` (vendor 32902 = 0x8086 Intel,
device 42920 = 0xA7A8), donc le device effectivement créé est l'Intel, alors
que le loader énumère bien une NVIDIA RTX 4070. Aucun chiffre « RTX 4070 »
n'est produit.

## Lignes brutes (stderr, `phase <nom> <ms>`, cumul depuis `main`)

### Série A

```text
[A1] phase w_fonts 18.5
[A1] phase w_instance 73.1
[A1] phase w_device 142.0
[A1] phase w_pipelines 147.8
[A1] phase main_ready 147.8
[A1] phase event_loop_ready 151.1
[A1] phase window 171.1
[A1] phase warm_joined 171.1
[A1] phase surface 177.2
[A1] phase vello_pipelines 177.2
[A1] phase text_layout 181.6
[A1] phase first_present 227.2
[A2] phase w_fonts 17.7
[A2] phase w_instance 58.5
[A2] phase w_device 109.1
[A2] phase w_pipelines 114.7
[A2] phase main_ready 114.8
[A2] phase event_loop_ready 116.5
[A2] phase window 132.5
[A2] phase warm_joined 132.5
[A2] phase surface 136.2
[A2] phase vello_pipelines 136.2
[A2] phase text_layout 139.3
[A2] phase first_present 182.5
[A3] phase w_fonts 19.0
[A3] phase w_instance 55.8
[A3] phase w_device 105.2
[A3] phase w_pipelines 111.6
[A3] phase main_ready 111.7
[A3] phase event_loop_ready 113.5
[A3] phase window 127.8
[A3] phase warm_joined 127.8
[A3] phase surface 129.9
[A3] phase vello_pipelines 129.9
[A3] phase text_layout 132.5
[A3] phase first_present 176.5
[A4] phase w_fonts 18.5
[A4] phase w_instance 55.3
[A4] phase w_device 108.7
[A4] phase w_pipelines 114.2
[A4] phase main_ready 114.2
[A4] phase event_loop_ready 116.2
[A4] phase window 132.7
[A4] phase warm_joined 132.8
[A4] phase surface 136.3
[A4] phase vello_pipelines 136.3
[A4] phase text_layout 139.0
[A4] phase first_present 185.8
[A5] phase w_fonts 23.9
[A5] phase w_instance 63.5
[A5] phase w_device 116.1
[A5] phase w_pipelines 121.6
[A5] phase main_ready 121.7
[A5] phase event_loop_ready 123.6
[A5] phase window 139.8
[A5] phase warm_joined 139.8
[A5] phase surface 142.2
[A5] phase vello_pipelines 142.2
[A5] phase text_layout 144.8
[A5] phase first_present 187.9
[A6] phase w_fonts 20.8
[A6] phase w_instance 60.4
[A6] phase w_device 111.5
[A6] phase w_pipelines 117.8
[A6] phase main_ready 117.8
[A6] phase event_loop_ready 119.7
[A6] phase window 132.6
[A6] phase warm_joined 132.6
[A6] phase surface 134.6
[A6] phase vello_pipelines 134.6
[A6] phase text_layout 137.2
[A6] phase first_present 180.4
```
### Série B

```text
[B1] phase main_ready 0.1
[B1] phase event_loop_ready 2.1
[B1] phase window 17.0
[B1] phase w_fonts 22.3
[B1] phase w_instance 62.6
[B1] phase w_device 119.3
[B1] phase w_pipelines 127.2
[B1] phase warm_joined 136.3
[B1] phase surface 138.7
[B1] phase vello_pipelines 138.7
[B1] phase text_layout 141.6
[B1] phase first_present 185.3
[B2] phase main_ready 0.1
[B2] phase event_loop_ready 2.1
[B2] phase window 17.9
[B2] phase w_fonts 18.0
[B2] phase w_instance 54.2
[B2] phase w_device 105.5
[B2] phase w_pipelines 111.0
[B2] phase warm_joined 121.0
[B2] phase surface 122.9
[B2] phase vello_pipelines 123.0
[B2] phase text_layout 125.8
[B2] phase first_present 169.3
[B3] phase main_ready 0.1
[B3] phase event_loop_ready 2.2
[B3] phase window 15.9
[B3] phase w_fonts 22.0
[B3] phase w_instance 55.1
[B3] phase w_device 104.3
[B3] phase w_pipelines 109.7
[B3] phase warm_joined 118.5
[B3] phase surface 120.7
[B3] phase vello_pipelines 120.7
[B3] phase text_layout 123.6
[B3] phase first_present 167.2
[B4] phase main_ready 0.1
[B4] phase event_loop_ready 2.3
[B4] phase window 17.0
[B4] phase w_fonts 17.5
[B4] phase w_instance 53.4
[B4] phase w_device 101.6
[B4] phase w_pipelines 106.9
[B4] phase warm_joined 116.2
[B4] phase surface 118.1
[B4] phase vello_pipelines 118.1
[B4] phase text_layout 121.0
[B4] phase first_present 164.3
[B5] phase main_ready 0.1
[B5] phase event_loop_ready 3.0
[B5] phase w_fonts 17.2
[B5] phase window 17.9
[B5] phase w_instance 58.6
[B5] phase w_device 116.9
[B5] phase w_pipelines 124.1
[B5] phase warm_joined 134.2
[B5] phase surface 136.7
[B5] phase vello_pipelines 136.7
[B5] phase text_layout 139.6
[B5] phase first_present 182.7
[B6] phase main_ready 0.1
[B6] phase event_loop_ready 2.1
[B6] phase window 16.4
[B6] phase w_fonts 21.7
[B6] phase w_instance 57.6
[B6] phase w_device 112.2
[B6] phase w_pipelines 117.7
[B6] phase warm_joined 127.3
[B6] phase surface 129.3
[B6] phase vello_pipelines 129.3
[B6] phase text_layout 132.2
[B6] phase first_present 175.1
```
### Série C

```text
[C1] phase main_ready 0.1
[C1] phase event_loop_ready 1.9
[C1] phase window 18.6
[C1] phase w_fonts 21.4
[C1] phase w_instance 57.6
[C1] phase w_device 110.5
[C1] phase w_pipelines 117.9
[C1] phase w_prewarm 165.9
[C1] phase warm_joined 175.1
[C1] phase surface 177.3
[C1] phase vello_pipelines 177.3
[C1] phase text_layout 180.2
[C1] phase first_present 184.7
[C2] phase main_ready 0.1
[C2] phase event_loop_ready 2.3
[C2] phase window 17.2
[C2] phase w_fonts 26.7
[C2] phase w_instance 59.6
[C2] phase w_device 110.7
[C2] phase w_pipelines 118.2
[C2] phase w_prewarm 171.3
[C2] phase warm_joined 180.7
[C2] phase surface 183.2
[C2] phase vello_pipelines 183.2
[C2] phase text_layout 186.0
[C2] phase first_present 190.5
[C3] phase main_ready 0.1
[C3] phase event_loop_ready 2.1
[C3] phase window 16.6
[C3] phase w_fonts 20.2
[C3] phase w_instance 56.5
[C3] phase w_device 114.2
[C3] phase w_pipelines 121.0
[C3] phase w_prewarm 174.7
[C3] phase warm_joined 184.3
[C3] phase surface 186.7
[C3] phase vello_pipelines 186.7
[C3] phase text_layout 189.6
[C3] phase first_present 194.0
[C4] phase main_ready 0.1
[C4] phase event_loop_ready 2.1
[C4] phase window 14.9
[C4] phase w_fonts 20.3
[C4] phase w_instance 56.8
[C4] phase w_device 110.3
[C4] phase w_pipelines 116.0
[C4] phase w_prewarm 167.7
[C4] phase warm_joined 177.3
[C4] phase surface 179.5
[C4] phase vello_pipelines 179.5
[C4] phase text_layout 182.3
[C4] phase first_present 187.0
[C5] phase main_ready 0.1
[C5] phase event_loop_ready 3.0
[C5] phase window 17.9
[C5] phase w_fonts 19.8
[C5] phase w_instance 58.2
[C5] phase w_device 109.6
[C5] phase w_pipelines 115.4
[C5] phase w_prewarm 166.8
[C5] phase warm_joined 176.3
[C5] phase surface 178.4
[C5] phase vello_pipelines 178.4
[C5] phase text_layout 181.4
[C5] phase first_present 185.9
[C6] phase main_ready 0.1
[C6] phase event_loop_ready 2.1
[C6] phase w_fonts 17.4
[C6] phase window 19.3
[C6] phase w_instance 58.5
[C6] phase w_device 111.5
[C6] phase w_pipelines 119.4
[C6] phase w_prewarm 170.1
[C6] phase warm_joined 179.8
[C6] phase surface 182.2
[C6] phase vello_pipelines 182.2
[C6] phase text_layout 185.1
[C6] phase first_present 189.6
```
### Série D

```text
[D1] phase main_ready 0.1
[D1] phase event_loop_ready 2.9
[D1] phase window 17.6
[D1] phase w_fonts 26.4
[D1] phase w_instance 57.5
[D1] phase w_device 131.6
[D1] phase w_pipelines 539.4
[D1] phase w_prewarm 604.3
[D1] phase warm_joined 613.7
[D1] phase surface 617.7
[D1] phase vello_pipelines 617.7
[D1] phase text_layout 620.5
[D1] phase first_present 625.3
[D2] phase main_ready 0.1
[D2] phase event_loop_ready 2.2
[D2] phase w_fonts 17.4
[D2] phase window 19.7
[D2] phase w_instance 59.9
[D2] phase w_device 123.9
[D2] phase w_pipelines 525.9
[D2] phase w_prewarm 584.5
[D2] phase warm_joined 594.1
[D2] phase surface 597.9
[D2] phase vello_pipelines 597.9
[D2] phase text_layout 600.7
[D2] phase first_present 607.0
[D3] phase main_ready 0.1
[D3] phase event_loop_ready 2.0
[D3] phase window 16.3
[D3] phase w_fonts 23.5
[D3] phase w_instance 58.4
[D3] phase w_device 126.1
[D3] phase w_pipelines 540.9
[D3] phase w_prewarm 605.2
[D3] phase warm_joined 614.8
[D3] phase surface 618.5
[D3] phase vello_pipelines 618.5
[D3] phase text_layout 621.3
[D3] phase first_present 625.9
[D4] phase main_ready 0.1
[D4] phase event_loop_ready 1.9
[D4] phase window 14.6
[D4] phase w_fonts 23.2
[D4] phase w_instance 56.9
[D4] phase w_device 122.0
[D4] phase w_pipelines 541.0
[D4] phase w_prewarm 609.4
[D4] phase warm_joined 618.7
[D4] phase surface 622.0
[D4] phase vello_pipelines 622.0
[D4] phase text_layout 624.8
[D4] phase first_present 629.3
[D5] phase main_ready 0.1
[D5] phase event_loop_ready 3.0
[D5] phase w_fonts 17.2
[D5] phase window 18.9
[D5] phase w_instance 54.3
[D5] phase w_device 117.1
[D5] phase w_pipelines 524.3
[D5] phase w_prewarm 584.3
[D5] phase warm_joined 593.5
[D5] phase surface 597.1
[D5] phase vello_pipelines 597.1
[D5] phase text_layout 599.9
[D5] phase first_present 604.5
[D6] phase main_ready 0.1
[D6] phase event_loop_ready 2.2
[D6] phase window 15.9
[D6] phase w_fonts 26.3
[D6] phase w_instance 57.4
[D6] phase w_device 118.6
[D6] phase w_pipelines 523.0
[D6] phase w_prewarm 579.5
[D6] phase warm_joined 588.9
[D6] phase surface 592.2
[D6] phase vello_pipelines 592.2
[D6] phase text_layout 595.1
[D6] phase first_present 612.0
```

## Tour complet des trois piles (étape 5)

`python3 round3/run_round3.py --stacks modular,egui,slint --runs 5` →
**PASS 3/3**, exit 0,
[`round3-20261126T165015Z/`](round3-20261126T165015Z/report.json).

| Pile | Verdict | 1re frame médiane (ms) | p50 | p95 | p99 | CPU/frame (ms) | RSS (Mio) | Binaire (o) |
|---|---|---|---|---|---|---|---|---|
| modular | PASS 5/5 | 201,8 | 4,17 | 4,26 | 4,36 | 1,03 | 187 | 17682416 |
| egui | PASS 5/5 | 176,0 | 4,17 | 4,27 | 4,44 | 0,89 | 185 | 23373824 |
| slint | PASS 5/5 | 94,2 | 4,16 | 4,31 | 4,84 | 1,76 | 136 | 29781600 |

| Pile | run1 | run2 | run3 | run4 | run5 |
|---|---|---|---|---|---|
| modular | 280,1 | 225,1 | 199,0 | 201,8 | 192,8 |
| egui | 192,9 | 168,6 | 176,0 | 177,4 | 171,3 |
| slint | 403,9 | 92,1 | 95,9 | 90,6 | 94,2 |

`build_s` enregistré : 0,4 s (modular), 0,4 s (egui), 0,5 s (slint) —
compilation incrémentale, non comparable à une compilation complète.

Le tour complet a été lancé **juste après la série D**, donc avec le cache de
shaders Mesa partiellement vide : la 1re exécution de chaque pile est à froid
(Slint 403,9 ms ; modular 280,1 ms ; egui 192,9 ms). Les médianes portent sur
les exécutions 2 à 5, chaudes. Le rapport du lanceur indique `"dirty": true`
parce que deux répertoires de résultats non suivis étaient présents dans le
workspace ; **aucun fichier suivi n'était modifié**.

## Limites

- **Une seule machine**, un seul OS, un seul pilote (Mesa/Intel), iGPU
  uniquement ; la série GPU dédié n'a pas pu être mesurée.
- « À chaud » n'est pas contrôlé au-delà des deux caches nommés : le cache
  Mesa avait été partiellement repeuplé par la série D ; les états exacts des
  caches avant chaque série n'ont pas été figés.
- n = 6 par série, aucun intervalle de confiance, aucun test statistique ;
  les écarts de 12–16 ms doivent être lus avec les plages min–max.
- La dérive thermique est réduite par l'entrelacement A/B/C, mais la série D
  est un bloc tardif (machine plus chaude) et le gouverneur CPU reste
  `powersave`, sans épinglage de fréquence.
- Phases **cumulées depuis `main`**, instrumentées dans le binaire : le
  lancement du processus et l'édition de liens dynamique avant `main` ne sont
  pas comptés.
- La frontière entre compilation de pipelines et premier rendu dépend du
  pilote (Mesa compile à la première utilisation) : B et C placent le même
  travail à des endroits différents, ce qui explique l'essentiel de leur écart.
- 5 frames par exécution : démarrage seulement, pas régime établi.
- Le tour complet a été mesuré à **échelle 133 %** : egui et Slint dessinent
  alors 1707 × 1067 pixels physiques contre 1280 × 800 pour la pile modulaire,
  ce qui biaise la comparaison inter-piles de ce tableau (pas les séries A–D).
- Pas mesurés : repos, latence d'entrée, VRAM, temps GPU par frame
  (protocole §5) ; texte long, IME, bidi, accessibilité (tour 4).
- Ces chiffres valent pour le commit `2495f98`, cette machine et cette
  session ; `TESTED`, pas `PROVED`.
