# Passe 3 — instance wgpu native, préchauffage optionnel — r1-pulse, 2026-11-26

Troisième étape d'UI-D21. Ce fichier ne contient que des mesures ;
**aucune recommandation**.

## Conditions

- Hôte : R1-Pulse, Ubuntu 26.04.1, noyau 7.0.0-44, session **Wayland**,
  i7-13620H, 14 GiB.
- GPU : **Intel Raptor Lake-P UHD** (Mesa 26.0.8, Vulkan 1.4.335). Aucune
  tentative de GPU dédié pour cette passe.
- Écran : Sharp LQ156T1JW04, 2560 × 1440 @ 239,97 Hz. **Séries E/F/G à
  l'échelle 133 %** ; **tour complet à l'échelle 100 %** (puis restauration à
  133 %, vérifiée stable). La pile force une surface de 1280 × 800 pixels
  physiques.
- Source : branche `bench-u1b-round3-report`, commit **`68055ef`**,
  `rustc 1.95.0 (59807616e 2026-04-14)`,
  `cargo build --release --locked -p r3-modular`.
- Alimentation : **secteur branché** (`ADP1 online=1`), batterie 96 % ;
  autres applications fermées, souris non touchée, fenêtre au premier plan.
- Scène : 10 000 lignes, 50 visibles, 5 frames par exécution
  (`R3_FRAMES=5`), phases sur stderr (`R3_PHASES=1`), temps cumulé depuis
  `main`. Commande de base :
  `R3_PHASES=1 R3_FRAMES=5 R3_OUT=/tmp/r3.json target/release/r3-modular`
- 6 exécutions par série, **18/18 en exit 0**, aucune sortie stderr hors
  lignes `phase`.

### Séries (entrelacées E1, F1, G1, E2, … pour limiter la dérive thermique)

| Série | Variables | Sens |
|---|---|---|
| **E** | `R3_ALL_BACKENDS=1 R3_PREWARM=1` | ancien comportement : tous les backends (Vulkan + GL), préchauffage |
| **F** | *(aucune)* | défaut passe 3 : API native seule (`Backends::PRIMARY`), sans préchauffage |
| **G** | `R3_PREWARM=1` | API native seule + préchauffage |

## Médianes par phase (temps cumulé depuis `main`, ms)

| Phase (cumul, ms) | E ancien (tous backends + préchauffage) | F passe 3 (natif, sans préchauffage) | G natif + préchauffage |
|---|---|---|---|
| polices (`w_fonts`) | 20,5 | 20,8 | 20,1 |
| instance wgpu (`w_instance`) | 63,2 | 45,0 | 46,5 |
| device (`w_device`) | 128,1 | 64,0 | 65,1 |
| pipelines Vello (`w_pipelines`) | 134,5 | 70,5 | 71,7 |
| préchauffage (`w_prewarm`) | 189,0 | — | 127,8 |
| `main` prêt (`main_ready`) | 0,1 | 0,1 | 0,1 |
| boucle d'événements prête | 2,2 | 2,0 | 2,2 |
| fenêtre | 17,1 | 16,5 | 16,9 |
| jointure du thread (`warm_joined`) | 198,1 | 70,6 | 127,9 |
| surface | 201,6 | 72,8 | 130,2 |
| `vello_pipelines` | 201,6 | 72,8 | 130,2 |
| mise en forme du texte | 205,2 | 75,7 | 133,3 |
| **première frame présentée** | 211,0 | 123,8 | 138,4 |

## Statistiques de la première frame

| Série | Médiane | Min | Max |
|---|---|---|---|
| E | 211,0 | 185,3 | 304,3 |
| F | 123,8 | 119,7 | 131,1 |
| G | 138,4 | 132,8 | 145,3 |

Décomposition (médianes) :
- **backends** (E → G, préchauffage constant) : 211,0 → 138,4 ms, soit
  **−72,6 ms** ; l'essentiel vient de `w_instance` (63,2 → 46,5) et de
  `w_device` (128,1 → 65,1) ;
- **préchauffage** (G → F, backends constants) : 138,4 → 123,8 ms, soit
  **−14,6 ms** pour la version sans préchauffage ;
- **E → F** (les deux changements) : 211,0 → 123,8 ms, soit **−87,2 ms**.

Faits relevés :

- en F, la compilation différée réapparaît après `text_layout`
  (75,7 → 123,8, soit ≈ 48 ms) ; en G le préchauffage la déplace dans le
  thread (71,7 → 127,8, soit ≈ 56 ms), et la première frame ne coûte plus que
  ≈ 5 ms après le texte (133,3 → 138,4) ;
- la médiane E cache une forte dispersion (185,3 à 304,3 ms, dont un cas à
  304,3) ; F et G sont resserrées (119,7–131,1 et 132,8–145,3) ;
- `w_fonts`, `event_loop_ready` et `window` sont stables dans les trois
  séries (≈ 20, 2 et 17 ms).

## Lignes brutes (stderr, `phase <nom> <ms>`, cumul depuis `main`)

### Série E

```text
[E1] phase main_ready 0.7
[E1] phase event_loop_ready 14.1
[E1] phase w_fonts 20.4
[E1] phase window 36.0
[E1] phase w_instance 83.2
[E1] phase w_device 186.7
[E1] phase w_pipelines 193.0
[E1] phase w_prewarm 281.7
[E1] phase warm_joined 291.3
[E1] phase surface 293.9
[E1] phase vello_pipelines 293.9
[E1] phase text_layout 297.7
[E1] phase first_present 304.3
[E2] phase main_ready 0.1
[E2] phase event_loop_ready 1.9
[E2] phase window 14.5
[E2] phase w_fonts 23.2
[E2] phase w_instance 83.3
[E2] phase w_device 147.9
[E2] phase w_pipelines 153.5
[E2] phase w_prewarm 217.0
[E2] phase warm_joined 226.5
[E2] phase surface 228.6
[E2] phase vello_pipelines 228.6
[E2] phase text_layout 231.5
[E2] phase first_present 236.2
[E3] phase main_ready 0.1
[E3] phase event_loop_ready 2.2
[E3] phase w_fonts 17.1
[E3] phase window 18.3
[E3] phase w_instance 60.6
[E3] phase w_device 114.9
[E3] phase w_pipelines 120.3
[E3] phase w_prewarm 178.2
[E3] phase warm_joined 187.5
[E3] phase surface 189.7
[E3] phase vello_pipelines 189.7
[E3] phase text_layout 192.6
[E3] phase first_present 197.6
[E4] phase main_ready 0.1
[E4] phase event_loop_ready 2.3
[E4] phase window 15.8
[E4] phase w_fonts 20.6
[E4] phase w_instance 63.9
[E4] phase w_device 124.8
[E4] phase w_pipelines 130.1
[E4] phase w_prewarm 185.8
[E4] phase warm_joined 195.0
[E4] phase surface 198.4
[E4] phase vello_pipelines 198.4
[E4] phase text_layout 202.9
[E4] phase first_present 209.7
[E5] phase main_ready 0.1
[E5] phase event_loop_ready 2.2
[E5] phase w_fonts 17.9
[E5] phase window 19.2
[E5] phase w_instance 62.5
[E5] phase w_device 131.3
[E5] phase w_pipelines 138.9
[E5] phase w_prewarm 192.2
[E5] phase warm_joined 201.2
[E5] phase surface 204.8
[E5] phase vello_pipelines 204.8
[E5] phase text_layout 207.6
[E5] phase first_present 212.3
[E6] phase main_ready 0.1
[E6] phase event_loop_ready 2.2
[E6] phase window 15.3
[E6] phase w_fonts 22.8
[E6] phase w_instance 57.0
[E6] phase w_device 107.2
[E6] phase w_pipelines 113.0
[E6] phase w_prewarm 165.8
[E6] phase warm_joined 175.3
[E6] phase surface 177.6
[E6] phase vello_pipelines 177.6
[E6] phase text_layout 180.6
[E6] phase first_present 185.3
```
### Série F

```text
[F1] phase main_ready 0.1
[F1] phase event_loop_ready 2.2
[F1] phase window 16.4
[F1] phase w_fonts 21.6
[F1] phase w_instance 46.1
[F1] phase w_device 69.2
[F1] phase w_pipelines 76.3
[F1] phase warm_joined 76.4
[F1] phase surface 78.6
[F1] phase vello_pipelines 78.6
[F1] phase text_layout 81.5
[F1] phase first_present 131.1
[F2] phase main_ready 0.1
[F2] phase event_loop_ready 2.0
[F2] phase window 19.8
[F2] phase w_fonts 20.7
[F2] phase w_instance 43.4
[F2] phase w_device 62.2
[F2] phase w_pipelines 69.3
[F2] phase warm_joined 69.4
[F2] phase surface 71.8
[F2] phase vello_pipelines 71.8
[F2] phase text_layout 74.7
[F2] phase first_present 122.9
[F3] phase main_ready 0.1
[F3] phase event_loop_ready 2.1
[F3] phase window 16.7
[F3] phase w_fonts 20.1
[F3] phase w_instance 47.3
[F3] phase w_device 65.7
[F3] phase w_pipelines 71.7
[F3] phase warm_joined 71.8
[F3] phase surface 73.8
[F3] phase vello_pipelines 73.8
[F3] phase text_layout 76.7
[F3] phase first_present 125.6
[F4] phase main_ready 0.1
[F4] phase event_loop_ready 2.0
[F4] phase window 15.2
[F4] phase w_fonts 21.1
[F4] phase w_instance 44.0
[F4] phase w_device 62.4
[F4] phase w_pipelines 68.5
[F4] phase warm_joined 68.6
[F4] phase surface 70.5
[F4] phase vello_pipelines 70.5
[F4] phase text_layout 73.4
[F4] phase first_present 121.4
[F5] phase main_ready 0.1
[F5] phase event_loop_ready 1.9
[F5] phase window 15.3
[F5] phase w_fonts 20.8
[F5] phase w_instance 48.4
[F5] phase w_device 67.0
[F5] phase w_pipelines 72.6
[F5] phase warm_joined 72.6
[F5] phase surface 74.6
[F5] phase vello_pipelines 74.6
[F5] phase text_layout 77.5
[F5] phase first_present 124.7
[F6] phase main_ready 0.1
[F6] phase event_loop_ready 2.3
[F6] phase w_fonts 17.8
[F6] phase window 18.4
[F6] phase w_instance 42.6
[F6] phase w_device 61.1
[F6] phase w_pipelines 68.1
[F6] phase warm_joined 68.2
[F6] phase surface 70.4
[F6] phase vello_pipelines 70.4
[F6] phase text_layout 73.3
[F6] phase first_present 119.7
```
### Série G

```text
[G1] phase main_ready 0.1
[G1] phase event_loop_ready 2.3
[G1] phase window 16.2
[G1] phase w_fonts 18.9
[G1] phase w_instance 43.5
[G1] phase w_device 62.1
[G1] phase w_pipelines 67.9
[G1] phase w_prewarm 127.1
[G1] phase warm_joined 127.3
[G1] phase surface 129.5
[G1] phase vello_pipelines 129.5
[G1] phase text_layout 132.3
[G1] phase first_present 137.3
[G2] phase main_ready 0.1
[G2] phase event_loop_ready 2.2
[G2] phase window 19.8
[G2] phase w_fonts 21.8
[G2] phase w_instance 43.2
[G2] phase w_device 61.8
[G2] phase w_pipelines 67.4
[G2] phase w_prewarm 125.4
[G2] phase warm_joined 125.6
[G2] phase surface 127.8
[G2] phase vello_pipelines 127.8
[G2] phase text_layout 130.7
[G2] phase first_present 135.2
[G3] phase main_ready 0.1
[G3] phase event_loop_ready 2.3
[G3] phase window 17.0
[G3] phase w_fonts 18.8
[G3] phase w_instance 52.8
[G3] phase w_device 71.2
[G3] phase w_pipelines 78.8
[G3] phase w_prewarm 135.3
[G3] phase warm_joined 135.4
[G3] phase surface 137.5
[G3] phase vello_pipelines 137.5
[G3] phase text_layout 140.3
[G3] phase first_present 145.3
[G4] phase main_ready 0.1
[G4] phase event_loop_ready 2.0
[G4] phase window 15.0
[G4] phase w_fonts 24.1
[G4] phase w_instance 43.7
[G4] phase w_device 62.1
[G4] phase w_pipelines 68.9
[G4] phase w_prewarm 122.7
[G4] phase warm_joined 122.9
[G4] phase surface 125.2
[G4] phase vello_pipelines 125.2
[G4] phase text_layout 128.0
[G4] phase first_present 132.8
[G5] phase main_ready 0.1
[G5] phase event_loop_ready 2.2
[G5] phase window 17.2
[G5] phase w_fonts 19.2
[G5] phase w_instance 49.4
[G5] phase w_device 68.1
[G5] phase w_pipelines 75.5
[G5] phase w_prewarm 129.4
[G5] phase warm_joined 129.5
[G5] phase surface 132.6
[G5] phase vello_pipelines 132.6
[G5] phase text_layout 135.6
[G5] phase first_present 140.2
[G6] phase main_ready 0.1
[G6] phase event_loop_ready 2.2
[G6] phase window 16.9
[G6] phase w_fonts 21.0
[G6] phase w_instance 50.9
[G6] phase w_device 68.9
[G6] phase w_pipelines 74.4
[G6] phase w_prewarm 128.4
[G6] phase warm_joined 128.5
[G6] phase surface 131.0
[G6] phase vello_pipelines 131.0
[G6] phase text_layout 134.3
[G6] phase first_present 139.5
```

## Tour complet des trois piles (échelle 100 %)

`python3 round3/run_round3.py --stacks modular,egui,slint --runs 5` →
**PASS 3/3**, exit 0,
[`round3-20261126T191706Z/`](round3-20261126T191706Z/report.json).
Rapport du lanceur : `"dirty": false`, caches chauds.

| Pile | Verdict | 1re frame médiane (ms) | p50 | p95 | p99 | CPU/frame (ms) | RSS (Mio) | Binaire (o) |
|---|---|---|---|---|---|---|---|---|
| modular | PASS 5/5 | 143,6 | 4,17 | 4,28 | 4,59 | 0,97 | 168 | 17682360 |
| egui | PASS 5/5 | 176,1 | 4,17 | 4,29 | 4,71 | 0,97 | 174 | 23373824 |
| slint | PASS 5/5 | 89,4 | 4,16 | 4,35 | 5,21 | 2,13 | 131 | 29781600 |

| Pile | run1 | run2 | run3 | run4 | run5 |
|---|---|---|---|---|---|
| Pile | run1 | run2 | run3 | run4 | run5 |
|---|---|---|---|---|---|
| modular | 158,3 | 142,4 | 143,6 | 142,4 | 144,6 |
| egui | 246,4 | 171,2 | 176,1 | 193,6 | 165,5 |
| slint | 91,4 | 84,1 | 92,4 | 89,4 | 87,9 |

`build_s` enregistré : 0,4 s (modular), 0,4 s (egui), 0,5 s (slint) —
compilation incrémentale, non comparable à une compilation complète.

## Limites

- **Une seule machine**, un seul OS, un seul pilote (Mesa/Intel), iGPU
  uniquement ; aucune mesure sur GPU discret.
- n = 6 par série : médianes et plages min–max seulement, aucun intervalle de
  confiance ni test statistique. La dispersion de E (185–304 ms) dépasse
  l'écart mesuré entre séries : l'effet « backends » est vu sur des médianes,
  pas sur des distributions appariées.
- E et F diffèrent par **deux** variables (backends et préchauffage) ; la
  décomposition E→G→F suppose que les effets s'additionnent, ce qui n'est pas
  vérifié ici.
- « Chaud » = caches wgpu et Mesa présents (repeuplés par les séries
  précédentes) ; leur état exact n'a pas été figé et l'ordre E/F/G interagit
  avec eux.
- Phases **cumulées depuis `main`**, instrumentées dans le binaire : le
  lancement du processus et l'édition de liens avant `main` sont exclus.
- La frontière compilation/présentation dépend du pilote (Mesa compile à la
  première utilisation) : c'est elle qui fait diverger F et G au premier
  rendu.
- 5 frames par exécution : démarrage seulement, pas régime établi.
- Le tour complet (100 %) n'est **pas** directement comparable aux tours
  précédents : le code a changé (`68055ef`) et les caches étaient chauds ici,
  alors que le tour de la passe 2 suivait une série à froid.
- Gouverneur CPU `powersave`, fréquence non épinglée.
- Pas mesurés : repos, latence d'entrée, VRAM, temps GPU par frame
  (protocole §5) ; texte long, IME, bidi, accessibilité (tour 4).
- Ces chiffres valent pour le commit `68055ef`, cette machine et cette
  session ; `TESTED`, pas `PROVED`.
