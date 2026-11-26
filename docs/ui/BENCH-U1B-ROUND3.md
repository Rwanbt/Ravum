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

## Première frame de la pile modulaire (UI-D21) : analyse

`R3_PHASES=1` écrit sur stderr le temps écoulé depuis `main` à chaque phase
du démarrage. Essai dans le conteneur (Xvfb, Vulkan **logiciel** lavapipe :
ordres de grandeur seulement, médiane de 3 exécutions, en ms) :

| Phase (cumul) | Avant | `area_only` |
|---|---|---|
| boucle d'événements prête | 37 | 37 |
| fenêtre | 38 | 38 |
| GPU : instance, adaptateur, device, surface | 72 | 55 |
| **pipelines Vello** (compilation des shaders) | **290** | **183** |
| mise en forme du texte (50 lignes) | 292 | 186 |
| premier rendu présenté | 451 | 316 |

Causes, par poids :
1. **Compilation des shaders de Vello** : la pile compilait les trois
   variantes d'anticrénelage (zone, MSAA 8, MSAA 16) alors que seule la
   zone sert. Corrigé : `AaSupport::area_only()` (≈ −110 ms ici).
   La traduction WGSL → SPIR-V par naga reste sur le chemin critique.
2. **Premier rendu** (≈ 130 ms ici) : premières soumissions au GPU ;
   à mesurer sur matériel réel.
3. **Démarrage à froid** : sur r1-pulse, la 1re exécution (293 ms) était
   plus lente que les suivantes (≈ 210 ms), signe du cache de shaders du
   pilote. Ajouté : cache de pipelines wgpu sur disque
   (`~/.cache/ravum-ui-bench/`, Vulkan seulement, écrit après la première
   frame ; `R3_NO_PIPELINE_CACHE` le coupe). lavapipe ne cache rien
   (fichier de 96 octets) : son effet n'est mesurable que sur un vrai pilote.
4. Texte, fenêtre et GPU : faibles (≈ 50 ms au total).

Pistes suivantes, à décider après la mesure sur r1-pulse : créer le device
et les pipelines dans un thread pendant la création de la fenêtre (≈ 35 ms) ;
shaders précompilés au build (supprime naga du démarrage, demande une
modification de Vello ou un fork).

`unsafe` : un bloc, `create_pipeline_cache`, justifié en commentaire
(données écrites par le seul programme, clé = adaptateur et pilote,
`fallback: true`). Code de banc, hors crates Ravum.

### Mesures r1-pulse (commit `86dc42e`) et deuxième étape

[`first-frame-2026-11-26.md`](../../bench/ui-candidates/results/r1-pulse/first-frame-2026-11-26.md)
corrige l'analyse ci-dessus : sur Mesa/Intel, les pipelines Vello coûtent
6 à 9 ms ; `area_only` ne gagne que ≈ 2 ms. À chaud, les 199 ms se
répartissent en trois postes **en série** : démarrage → boucle d'événements
(70 ms, dont l'instance wgpu), fenêtre (15), adaptateur + device + surface
(56), puis premier rendu (47, compilation différée des pipelines par le
pilote, confirmée ici : le rendu de préchauffage la déplace).

Deuxième étape (ce commit) : tout ce qui ne demande pas la fenêtre est
préparé **dans un thread dès `main`**, pendant que la boucle d'événements et
la fenêtre se créent : instance, adaptateur et device wgpu, pipelines Vello,
un rendu de préchauffage hors écran (64 × 64), et la base de polices sur un
second thread. Au moment de la fenêtre, il ne reste que la surface.
Fail-closed : si le device préparé ne peut pas présenter sur la fenêtre, un
device et un renderer compatibles sont créés à la place.
Variables pour A/B : `R3_SERIAL_INIT` (même travail, sans parallélisme),
`R3_NO_PREWARM`. Nouvelles phases : `w_instance`, `w_device`,
`w_pipelines`, `w_prewarm`, `w_fonts`, `warm_joined`, `surface`.
Sous lavapipe (CPU), le gain est faible : le thread est le chemin critique.
Il est attendu sur un vrai GPU, où la boucle et la fenêtre (≈ 85 ms)
recouvrent le GPU et les polices ; à mesurer.

### Passe 2 sur r1-pulse (commit `2495f98`) et troisième étape

[`first-frame-2-2026-11-26.md`](../../bench/ui-candidates/results/r1-pulse/first-frame-2-2026-11-26.md) :
parallèle sans préchauffage (B) 172 ms contre 184 ms en série (A) ; la
fenêtre est prête à 17 ms, mais le **thread de préparation** est désormais
le chemin critique : instance wgpu 56 ms, adaptateur + device 50 ms. Le
préchauffage (C) coûte 50 ms au thread et n'en rend que 43 : 188 ms.
À froid réel (caches wgpu et Mesa vidés, D) : 619 ms, dont ≈ 410 ms de
compilation de shaders par le pilote.

Troisième étape (ce commit) :
- instance wgpu limitée à l'API native (`Backends::PRIMARY` : Vulkan, Metal,
  DX12) au lieu de toutes ; `RenderContext::new` demande aussi GL, dont
  l'initialisation (EGL) charge les pilotes GL de toutes les cartes, NVIDIA
  comprise, pour un adaptateur jamais choisi. `WGPU_BACKEND` garde la main ;
  `R3_ALL_BACKENDS` rétablit l'ancien ensemble (A/B) ;
- préchauffage désactivé par défaut (`R3_PREWARM` pour le mesurer).
Sans effet mesurable dans le conteneur (aucun pilote GL) ; à mesurer.
Reste ouvert : le froid réel (≈ 410 ms de compilation par le pilote, une
fois par installation ou mise à jour du pilote).

### Passe 3 sur r1-pulse (commit `68055ef`) : bilan

[`first-frame-3-2026-11-26.md`](../../bench/ui-candidates/results/r1-pulse/first-frame-3-2026-11-26.md) :
première frame de la pile modulaire, médiane à chaud, **124 ms** (119,7 à
131,1) avec le réglage par défaut, contre 199 ms au départ (passe 1) et
211 ms avec l'ancien ensemble d'API (série E). L'API native seule gagne
≈ 73 ms (instance et device) ; le préchauffage reste une perte (+15 ms).
Tour complet à l'échelle 100 % : modulaire 144 ms, egui 176 ms, Slint 89 ms.

Ce qui reste sur le chemin critique (F) : thread de préparation 71 ms
(instance 45, device 19, pipelines 6), puis ≈ 48 ms de compilation différée
par le pilote au premier rendu. Non résolu : le froid réel (≈ 410 ms de
compilation par le pilote après installation ou mise à jour du pilote).
Les blocs de lignes brutes de ce fichier sont vides : seules les médianes
et les plages min–max ont été transmises.
