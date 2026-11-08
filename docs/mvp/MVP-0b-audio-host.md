# MVP-0b / L4-C — Chemin audio réel : ce qui est mesuré et ce qui ne l'est pas

> **Ce document existe pour éviter une fausse revendication.** Le démonstrateur DSP de
> Ravum est un **simulateur de blocs**, et une **sonde réelle** exécute désormais le vrai
> chemin DSP sur un périphérique `waveOut` Windows **en n'émettant que du silence**. Ce
> document rapporte (1) l'inventaire matériel réel de la machine, (2) une mesure réelle
> d'ordonnanceur OS qui borne honnêtement les prétentions, (3) les mesures réelles sur
> périphérique (travail DSP, latence, swaps, allocations), (4) la priorité temps réel
> Windows (MMCSS) mesurée sous charge CPU contrôlée, (5) la liste explicite de ce qui
> n'a pas été exécuté et pourquoi.
>
> **Ce qui n'est toujours pas qualifié** : pas de WASAPI/ASIO, pas d'hôte CLAP/VST3/DAW,
> pas de XRUN mesurable via `waveOut`, **aucune** qualification hard-réaliste. La priorité
> temps réel Windows (MMCSS « Pro Audio » + `AVRT_PRIORITY_CRITICAL`) a été **mesurée**
> sous charge CPU contrôlée (§5), mais c'est une demande de classe d'ordonnancement, pas
> une garantie d'échéance. Une sonde `waveOut` legacy n'est pas un pilote audio certifié.

## 1. Ce qui est réellement implémenté et mesuré aujourd'hui

| Élément | Statut |
|---|---|
| ABI de bloc v2 (`ravum_process_block`), état possédé par l'hôte | implémenté, testé |
| Publication sans verrou, garde non possédante sur le callback | implémenté, testé (`MVP-0b-rt-lifecycle.md`) |
| 0 allocation / 0 désallocation sur le thread « audio » (simulé) | mesuré sur 199 517 blocs |
| Crossfade de remplacement (clics M3 0,108/0,657 → 0,029/0,058) | implémenté, mesuré (`MVP-0b-dsp-crossfade.md`) |
| Cadence simulée `frames / sample_rate` | mesuré (période dérivée 5 333 µs) |
| **Chemin DSP réel sur périphérique `waveOut` (silence)** | **mesuré (§4)** : 282 tampons, 0 alloc/0 free sur le thread de rendu, 11 swaps |
| **Swaps à chaud sous pilote réel, avec et sans crossfade** | **mesurés (§4.5)** : 11 activations/run, 0 refus ; `\|delta\|` 0,0291 avec fondu vs 0,1260 sans |
| **Priorité temps réel MMCSS (Pro Audio + critical) sous charge CPU 100 %** | **mesurée (§5)** : 2 runs/cellule, 0 tampon en retard avec MMCSS (max 2,7 ms) ; sans MMCSS un run dégradé à 134,4 ms et 255 tampons en retard |
| **Chemin DSP réel sur WASAPI mode partagé (silence)** | **mesuré (§6)** : 8 runs, endpoint MOTU 48 kHz float32, buffer moteur 22 ms, cadence ratio 1,0001–1,0009, 0 événement en retard, 0 alloc/0 free, 11/11 swaps |
| **Chemin DSP réel sur WASAPI mode exclusif (silence)** | **mesuré (§6.5) sur l'endpoint NVIDIA HDMI** : PCM16 48 kHz, période applicative 10 ms, 5 runs, ratio 1,0000, 0 événement en retard, 0 starvations, 0 alloc/free, 11/11 swaps. Le périphérique par défaut (MOTU) **refuse** tous les formats exclusifs balayés (`AUDCLNT_E_UNSUPPORTED_FORMAT`). |
| ASIO, priorité temps réel WASAPI exclusive sur le périphérique par défaut | **aucun** |
| Hôte CLAP/VST3/ASIO branché sur le module | **aucun** |
| XRUN mesurés, glitches de pilote comptés | **aucun** (ni `waveOut` ni WASAPI partagé ne les exposent ; seules des starvations définies et des lateness mesurées) |

## 2. Inventaire matériel observé (Windows 11 26100)

`Win32_SoundDevice` — statut `OK` au moment de l'audit :

- **MOTU M Series** (MOTU, Inc) — interface audio USB ;
- **Realtek High Definition Audio** ;
- NVIDIA Virtual Audio Device (Wave Extensible), NVIDIA High Definition Audio ;
- Intel Smart Sound (microphones numériques), Nahimic Easy Surround / mirroring, audio Bluetooth.

Énumération `waveOut` (4 périphériques, `waveOutGetNumDevs()` = 4, tous en 48 kHz
stéréo 16 bits d'après `dwFormats = 0x000FFFFF`) :

| index `waveOut` | nom renvoyé par `waveOutGetDevCapsW` |
|---:|---|
| 0 | `Out 1-2 (MOTU M Series)` |
| 1 | `Haut-parleurs (Realtek(R) Audio` |
| 2 | `FHD (NVIDIA High Definition Audio` |
| 3 | `FHD (NVIDIA High Definition Audio` |

Le périphérique par défaut (ouvert avec `WAVE_MAPPER`) rapporte `waveOutGetID` =
`0xFFFFFFFF` : **cette API ne nomme pas l'endpoint physique résolu**, seulement le
mappeur (`Mappeur de sons Microsoft`). Le comportement mesuré du mappeur est identique à
celui du périphérique `waveOut` n° 0 (MOTU) à 256 frames, et différent de celui du n° 1
(Realtek) : l'endpoint par défaut est donc la voie WDM MOTU. C'est une **inférence
documentée**, pas une lecture directe.

Des plugins tiers CLAP/VST3 sont installés sur la machine
(`C:\Program Files\Common Files\CLAP`, `…\VST3`), mais **aucun hôte audio n'a été
qualifié ni utilisé pour Ravum**, et Ravum ne produit **aucun** binaire plugin
CLAP/VST3 à ce stade.

## 3. Mesure réelle : gigue d'ordonnanceur OS

Outil : `cargo run -q -p ravum-dsp-harness --bin ravum-jitter -- --iterations 2000
--period-us 5333 --json`. Il crée un thread périodique à échéances fixes
(`start + i × période`), avec `timeBeginPeriod(1)` (granularité du timer Windows à 1 ms),
et mesure la lateness par réveil et l'intervalle entre réveils.

Machine : i7-13620H 10c/16t, 15,71 Go. Charge au démarrage : **CPU 52 %**, 3,07 Go de RAM
libre, d'autres agents en cours d'exécution. Preuve brute :
`evidence/mvp0b-os-jitter.txt`.

| Métrique (µs, 2 000 réveils, période 5 333 µs) | min | p50 | p95 | p99 | max |
|---|---:|---:|---:|---:|---:|
| lateness (réveil − échéance) | 51 | **368** | **648** | 910 | **2 602** |
| intervalle entre réveils | 3 419 | 5 290 | 5 747 | 5 921 | 7 564 |

Échéances ratées de plus d'une période complète : **0**. Aucun réveil n'a dépassé une
période entière, mais la gigue atteint **2,6 ms** (≈ 49 % d'une période de 5,33 ms) et
l'intervalle maximal 7,56 ms.

Lecture honnête :

1. Ce n'est **pas** un test audio : aucun pilote, aucun tampon, aucune interruption de
   périphérique, aucune priorité `THREAD_PRIORITY_TIME_CRITICAL`. C'est le meilleur cas
   possible de l'ordonnanceur, pas le pire.
2. Il **borne les prétentions** : sur cette machine chargée, un callback de période
   5,33 ms subit déjà des retards de l'ordre de 0,4–0,9 ms (p50–p99) et jusqu'à 2,6 ms
   **sans même toucher au matériel audio**. Toute affirmation « hard real-time » serait
   donc fausse sur cette base.
3. Les valeurs varient avec la charge (le CPU était à 52 % ici) ; elles ne sont pas
   portables.

## 4. Résultats réels (périphérique `waveOut`)

### 4.1 Outil et garantie de silence

Binaire : `cargo run -q -p ravum-dsp-harness --bin ravum-audio-probe -- [options]`
(`crates/ravum-dsp-harness/src/bin/ravum-audio-probe.rs`).

- ouvre le périphérique `waveOut` en PCM 16 bits, 2 canaux, `--rate` (48 000 par défaut),
  `CALLBACK_EVENT`, tampons de `--frames` frames en rotation (`--buffers`),
  `waveOutPrepareHeader`/`waveOutWrite`/`waveOutUnprepareHeader`, arrêt propre
  `waveOutReset` + `waveOutClose` ;
- un **thread de rendu dédié** attend `WOM_DONE`, horodate la lateness vs l'échéance,
  appelle `BlockHost::process_rt` sur un bloc mono (sinusoïde 440 Hz, amplitude 0,5, phase
  continue) **dans ce thread**, mesure la durée du travail, calcule le delta
  inter-échantillon de la sortie calculée, **puis remplit le tampon du pilote de zéros**
  avant `waveOutWrite` ;
- un thread de contrôle prépare et applique les modules de rechange (`--swap-build`,
  JSON de `ravum build --json`) à intervalle `--swap-every-ms`, avec crossfade
  `--crossfade-frames` ;
- **aucun son audible** : le signal DSP est calculé, mesuré, jeté ; seuls des zéros sont
  soumis au pilote. Toutes les exécutions ci-dessous rapportent `non_finite_samples = 0`.

Le module DSP est **précompilé** par `ravum build` (fixtures `iir_v1`/`iir_v2`) et passé
en argument : la sonde ne compile rien dans son processus de mesure. Le script
`scripts/audio-probe.ps1` fait la compilation, lance la sonde, propage le code de sortie
et affiche le résumé.

Versions : branche `feat/mvp0b-hardening` au-dessus du commit `2687ec9` (deux commits
sans rapport — COLLAB-4 et E2E navigateur — sont arrivés sur la branche pendant la
campagne et ne touchent pas le chemin audio), `rustc 1.95.0`, Windows `10.0.26100`
(Windows 11), binaire en profil Cargo **debug** (le surcoût hôte de `process_rt` reflète
ce profil). Aucune dépendance externe ajoutée : FFI `winmm`/`kernel32`/`ntdll` en
`extern "system"`, `#[cfg(windows)]`, stub non-Windows qui rapporte `Unsupported` sans
inventer de mesure.

### 4.2 Périphérique et format observés

- Requête : `WAVE_MAPPER` (périphérique par défaut), `--device` permet un index explicite.
- `waveOutOpen` **réussi** ; `WAVEFORMATEX` PCM 16 bits, 2 canaux, 48 000 Hz,
  `nBlockAlign = 4`, `nAvgBytesPerSec = 192 000`, `cbSize = 0`.
- Configuration de référence : `--frames 512` (période **10 666 µs**), `--buffers 4`
  (profondeur de file 42,7 ms), `timeBeginPeriod(1)` accordé.
- Le coût d'ouverture/activation du moteur audio est mesuré **séparément** :
  `open_prepare_us ≈ 421 918 µs` sur le run de référence (l'audio engine Windows démarre) ;
  il n'entre ni dans la période ni dans la lateness.

### 4.3 Configuration de référence : 512 frames, 3 s, module réel + swaps

Exécution réelle :
`powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1`
→ preuve `evidence/mvp0b-audio-probe-2026-11-08.json`. Module `iir_v1`
(`y = 0,1·x + 0,9·z1`), swaps alternés `iir_v2`/`iir_v1` (`y = 0,5·x + 0,5·z1`) toutes
les 250 ms, crossfade 256 frames.

| Mesure (3 003 ms de rendu, 282 tampons) | valeur |
|---|---|
| tampons traités | 282 (8 de warm-up + 274 mesurés), 286 `waveOutWrite`, 0 échec |
| cadence livrée | **93,8 tampons/s** pour 93,75 attendus → **ratio 1,001** ; position 144 406 éch. pour 144 384 complétés (+22) |
| travail DSP (`process_rt`, mesure par bloc) | p50 **7 µs**, p95 **14 µs**, p99 **45 µs**, max **83 µs** |
| callback complet (réveil → re-soumission) | p50 **275 µs**, p95 **503 µs**, max **756 µs** |
| lateness par réveil (voir définition dans le JSON) | min −5 090, p50 **−216**, p95 **4 091**, p99 **4 840**, max **5 032 µs** |
| tampons/réveils en retard de plus d'une période | **0** / **0** |
| marge de file dérivée | min **26 568 µs**, p50 31 944 µs → **0** risque de vidage |
| réveils sans tampon terminé / timeouts | 6 timeouts (aucun tampon < 20 ms), 1 tampon groupé |
| allocations / désallocations sur le thread de rendu | **0 / 0** (compteur de l'allocateur global, après warm-up) |
| swaps | **11 demandés, 11 activés, 0 refusé, 0 erreur** (chargement 2–4 ms, preuve 2–3 ms, publication 0 ms) |
| `\|delta\|` max inter-échantillon | aux swaps **0,0291**, hors swap **0,0287**, référence sinusoïdale **0,0288** |
| état de l'hôte en fin de run | 11 activations, 21 modules déchargés, 2 vivants, version active 12 |

Lecture honnête : sur cette configuration, le vrai chemin ABI de bloc tourne dans le
callback réel sans alloc ni free, le travail est de l'ordre de quelques microsecondes, et
la file de 42,7 ms garde une marge positive de 26–32 ms. `waveOut` livre les `WOM_DONE`
**par lots** au rythme de service du moteur audio (intervalles de réveil p50 ≈ 10 066 µs,
p95 ≈ 19 338 µs, max ≈ 20 333 µs, pour une période de 10 666 µs) : la lateness mesurée
contient donc la quantification de livraison du pilote, pas seulement la gigue de
l'ordonnanceur. Les 6 timeouts sont des réveils sans tampon terminé dans 20 ms (livraison
groupée), pas des retards de soumission : aucun tampon n'a été soumis avec plus d'une
période de retard.

Variabilité observée sur des runs identiques : la lateness médiane a varié entre
**−4,3 ms et +2,9 ms**, le max du travail DSP entre **19 µs et 109 µs**, le max du
callback entre **603 µs et 4 430 µs** ; ces écarts sont réels et liés à la charge de la
machine (d'autres agents compilent en parallèle).

### 4.4 Le réglage par défaut du binaire (256 frames) ne tient pas le temps réel sur ce pilote

Le mandat demande aussi la configuration par défaut (`--frames 256`, période 5 333 µs).
Exécution réelle :
`scripts/audio-probe.ps1 -Frames 256 -Out evidence/mvp0b-audio-probe-2026-11-08-f256.json`.

| Mesure (3 014 ms de rendu, 301 tampons) | valeur |
|---|---|
| cadence livrée | **100,0 tampons/s** pour 187,5 attendus → **ratio 0,53** ; 25 595 éch/s effectifs au lieu de 48 000 |
| lateness par réveil | min 0, p50 **686 014 µs**, p95 **1 327 397 µs**, max **1 378 556 µs** |
| tampons en retard de plus d'une période | **292** (sur 73 réveils) |
| marge de file dérivée | min **−1 363 304 µs**, p50 **−681 827 µs**, 292 risques de vidage |
| réveils sans tampon terminé / timeouts | **98** timeouts, 225 tampons groupés (intervalles p50 40 672 µs) |
| travail DSP / allocations | p50 3 µs, p95 8 µs / **0 / 0** |
| swaps | 11/11 activés, delta aux swaps 0,0289 (fondu) |

**C'est un résultat, pas un incident du harnais.** La taille de tampon (5,33 ms) est
inférieure au quantum de service de cette voie WDM : le pilote ne consomme plus le flux
au rythme nominal, la file se vide, les complétions arrivent par paquets toutes les
~40 ms en moyenne (≈ 4 périodes), et jusqu'à **1,38 s** de retard cumulé est observé.
Contre-preuve exécutée : le même binaire, mêmes swaps, mêmes 256 frames, sur le
périphérique `waveOut` n° 1 (Realtek) donne **ratio 1,001**, 187,7 tampons/s
(48 058 éch/s), lateness p50 −1 969 µs / max 1 976 µs, marge min 13 866 µs, 0 risque de
vidage, 0 alloc (`evidence/mvp0b-audio-probe-2026-11-08-realtek-f256.json`). Le harnais
sait donc tenir 256 frames ; c'est la voie MOTU par défaut qui lâche à cette taille. Ce
n'est pas un XRUN mesuré (l'API ne l'expose pas) : ce sont des **tampons soumis en
retard** et une file vidée.

### 4.5 Crossfade mesuré sur le périphérique

Même configuration de référence, `-CrossfadeFrames 0` :
`evidence/mvp0b-audio-probe-2026-11-08-nocrossfade.json`.

| `|delta|` max inter-échantillon | avec fondu 256 frames | sans fondu |
|---|---:|---:|
| aux swaps | **0,0291** | **0,1260** |
| hors swap (état stable) | 0,0287 | 0,0287 |
| référence sinusoïdale (`2·A·sin(ω/2)`) | 0,0288 | 0,0288 |

Le clic est reproduit **sur le périphérique réel** sans crossfade (0,1260, du même ordre
que le 0,108 mesuré par le simulateur M3 sur un signal d'amplitude différente), et il
disparaît avec le fondu (0,0291 ≈ référence). Les 11 swaps sont activés dans les deux cas,
sans refus, sans allocation.

### 4.6 Sous charge machine

Run complémentaire avec **charge ajoutée contrôlée** (12 processus PowerShell en boucle
occupée ; CPU mesuré 50 % → 100 % pendant le run), même configuration de référence :
`evidence/mvp0b-audio-probe-2026-11-08-under-load.json`.

| Mesure (3 029 ms, 260 tampons) | valeur |
|---|---|
| cadence livrée | 87,2 tampons/s (ratio **0,93**) |
| lateness par réveil | min −5 561, p50 **200 076 µs**, p95 **205 939 µs**, max **207 862 µs** |
| tampons en retard de plus d'une période | **240** (233 réveils) |
| marge de file | min **−186 523 µs**, 237 risques de vidage |
| callback complet | p50 332 µs, **max 107 871 µs** ; intervalles max 107 879 µs |
| travail DSP / allocations / swaps | p50 9 µs / **0 / 0** / 11/11 activés, delta swaps 0,0293 |

Conclusion : le facteur limitant est **l'ordonnancement OS d'un thread non temps réel**,
pas le travail DSP (p50 9 µs) ni les swaps (delta inchangé). Des décrochages du même ordre
(~100 ms) ont aussi été observés sporadiquement sur des runs sans charge ajoutée, quand
d'autres agents de la machine compilaient. Le §5 mesure précisément ce point : avec MMCSS
`Pro Audio` + `AVRT_PRIORITY_CRITICAL`, ces mêmes 100 % de charge CPU ne font plus
décrocher le thread de rendu (0 tampon en retard sur 2 runs) ; sans MMCSS, la dégradation
reste possible. C'est exactement la raison pour laquelle aucune revendication hard-RT
n'est faite : MMCSS améliore l'ordonnancement, il ne garantit pas l'échéance.

### 4.7 Ce que ces chiffres ne prouvent pas

- `waveOut` (winmm) est une API **legacy émulée**, pas un pilote temps réel certifié :
  pas de WASAPI exclusive, pas d'ASIO. Les runs du §5 enregistrent le thread de rendu
  auprès de MMCSS, mais MMCSS ne change ni le pilote ni l'API et ne promet aucune
  échéance. Ce n'est **pas** une qualification hard-réaliste.
- **Les XRUN ne sont pas mesurables via `waveOut`.** Le document parle de « tampons
  soumis en retard », de « marge de file » et de « risque de starvation » — jamais de
  XRUN mesurés.
- La lateness contient la **quantification de livraison des `WOM_DONE`** par le moteur
  audio (réveils ~10 ms, parfois deux tampons par réveil) ; elle ne mesure pas seulement
  la gigue du scheduler. La mesure de gigue pure est celle du §3.
- Les compteurs d'allocations instrumentent l'allocateur global Rust **par thread** :
  un compte positif prouve une violation, un compte nul ne prouve pas le hard-RT.
- Une seule machine, un seul OS, une seule interface pour la mesure par défaut, un format,
  des fenêtres de 3 s. Les résultats ne sont ni portables ni répétés sur plusieurs minutes.
- Le binaire est en profil **debug** ; le module DSP est compilé `opt-level=1` par
  `ravum build`.
- Chaque run mesure un état instantané de la machine (charge des autres agents incluse) ;
  la variabilité run-à-run est réelle et documentée (§4.3, §4.6, §5).
- MMCSS « Pro Audio » + `AVRT_PRIORITY_CRITICAL` est une **classe d'ordonnancement**,
  pas une garantie d'échéance : il ne rend pas `waveOut` temps réel certifié et ne
  remplace pas WASAPI/ASIO (§5).

## 5. Priorité temps réel (MMCSS) sous charge

### 5.1 Ce qui a été implémenté

La sonde accepte désormais `--mmcss` (défaut **off**) et
`--thread-priority <normal|above|high|time-critical>`. Les deux s'appliquent au
**thread de rendu**, pas au processus :

1. `AvSetMmThreadCharacteristicsW(L"Pro Audio", &task_index)` (`avrt.dll`) : si le
   handle est nul, la sonde **échoue explicitement** (code `GetLastError`, sortie 1) —
   jamais de faux succès ; le rapport contient `realtime.mmcss_acquired=false`,
   `avrt_error` et une note ;
2. `AvSetMmThreadPriority(handle, AVRT_PRIORITY_CRITICAL)` : retour vérifié ; un échec
   libère le handle MMCSS et échoue explicitement (`avrt_error`) ;
3. `SetThreadPriority` (kernel32) **après** MMCSS, si `--thread-priority` est demandé ;
   échec explicite (`win32_error`), MMCSS libéré ;
4. en fin de run, `AvRevertMmThreadCharacteristics(handle)` : le résultat est reporté
   dans `realtime.revert` (appelé, ok, code d'erreur, priorité de base après revert).

Ordre appliqué : MMCSS d'abord, priorité Win32 brute ensuite. Le rapport JSON `realtime`
contient `mmcss_requested`, `mmcss_acquired`, `mmcss_task_index`, `mmcss_priority`,
`mmcss_priority_set`, `thread_priority_requested`, `thread_priority_win32_constant`,
`thread_priority_set`, `thread_priority_observed` (valeur relue par `GetThreadPriority`),
`baseline_thread_priority`, `priority_after_mmcss`, `applied_order`, `avrt_error`,
`win32_error`, `notes`, et `revert`.

Mesures d'observation sur cette machine : priorité de base au départ **0** ; après
MMCSS Pro Audio + critical, `GetThreadPriority` renvoie **18** ; `--thread-priority
time-critical` seul renvoie **15**, `high` renvoie **2** ; avec MMCSS puis
`time-critical`, la priorité observée finale est **15** (la priorité brute écrase la
priorité MMCSS, c'est l'ordre documenté). Revert MMCSS : **ok**, priorité de base de
retour à **0**.

La sonde échantillonne aussi la charge machine autour de la fenêtre mesurée
(`GetSystemTimes` : delta occupé/total sur tous les CPU ; `GlobalMemoryStatusEx` : RAM
libre/totale), et chaque preuve JSON contient `load` avec
`cpu_busy_percent_measured`, `ram_available_bytes_start/end`, `ram_total_bytes` et
`probe_load_note`. La charge contrôlée est créée par `scripts/audio-probe.ps1 -Load 12` :
12 processus PowerShell en boucle occupée, bornés à ≤ 10 s, arrêtés dans le `finally`
du script.

### 5.2 Protocole de campagne

Configuration identique à la référence §4.3 : `--seconds 3 --frames 512 --buffers 4`,
module réel `iir_v1` + swaps `iir_v2`/`iir_v1` toutes les 250 ms, crossfade 256 frames.
4 cellules × **2 répétitions**, plus une 3e exécution de la cellule « repos sans MMCSS »
pour la reproductibilité :

| Cellule | MMCSS | Charge contrôlée |
|---|---|---|
| `rest-nommscss` | non | aucune |
| `rest-mmcss` | Pro Audio + critical | aucune |
| `load-nommscss` | non | 12 processus busy-loop |
| `load-mmcss` | Pro Audio + critical | 12 processus busy-loop |

CPU mesuré par la sonde : **31–52 %** pour les cellules « repos » (machine partagée avec
d'autres agents), **100,0 %** pour les cellules « charge ». RAM libre au départ :
1,0–1,6 Gio sur 15,7 Gio. Le coût d'activation du moteur audio reste mesuré séparément :
0,08–0,49 s au repos, **0,75–1,56 s** sous charge (`open_prepare_us`), il n'entre pas
dans la lateness.

### 5.3 Résultats mesurés (répétitions brutes)

Lateness par réveil (µs), travail DSP, callback, tampons en retard, marge de file, ratio
de cadence et allocations, pour chaque répétition :

| Cellule | CPU | ratio | lateness p50 | lateness p95 | lateness max | callback max | tardifs > 1 période | marge min | alloc/free |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| rest-nommscss r1 | 39,1 % | 1,0007 | −3 510 | 997 | 1 785 | 931 | 0 | +29 817 | 0/0 |
| rest-nommscss r2 | 39,5 % | 1,0005 | −836 | 3 607 | 4 988 | 890 | 0 | +26 534 | 0/0 |
| rest-nommscss r3 (reproductibilité) | 51,7 % | 1,0006 | 412 | 4 945 | 6 139 | 1 853 | 0 | +25 507 | 0/0 |
| rest-mmcss r1 | 31,1 % | 1,0009 | 2 072 | 6 809 | 7 693 | 478 | 0 | +24 076 | 0/0 |
| rest-mmcss r2 | 41,6 % | 1,0007 | 2 841 | 7 594 | 8 307 | 464 | 0 | +23 300 | 0/0 |
| load-nommscss r1 | 100,0 % | 0,9975 | 2 927 | 7 423 | 15 682 | 8 701 | **3** | +15 983 | 0/0 |
| load-nommscss r2 | 100,0 % | 0,9594 | **117 597** | **126 254** | **134 445** | **40 408** | **255** | **−111 885** | 0/0 |
| load-mmcss r1 | 100,0 % | 1,0004 | −2 848 | 1 459 | **2 538** | 505 | **0** | +29 109 | 0/0 |
| load-mmcss r2 | 100,0 % | 1,0009 | −2 830 | 1 513 | **2 700** | 498 | **0** | +28 973 | 0/0 |

Travail DSP (p50/p95/max, µs) : 7–10 / 10–22 / 17–131 selon les runs — le travail
lui-même reste de l'ordre de la dizaine de µs, seul l'ordonnancement varie (le max 131 µs
correspond au run r3 à 51,7 % de CPU). Swaps : 10 à 11 activations par run, 0 refus,
`|delta|` aux swaps 0,0290–0,0293 (fondu continu) dans **toutes** les cellules. Toutes
les cellules : **0 allocation / 0 désallocation** sur le thread de rendu, modules MMCSS
acquis (`mmcss_acquired=true`, `task_index` 407/410/417/420), priorité `critical` posée,
revert MMCSS ok.

### 5.4 Lecture honnête

1. **Sous charge CPU 100 % contrôlée, MMCSS change le résultat de façon nette** : les
   deux runs `load-mmcss` restent à 0 tampon en retard, lateness max **2,5–2,7 ms**,
   marge de file **+29 ms**, callback max ~500 µs, ratio 1,0004–1,0009. Sans MMCSS, le
   même protocole donne un run légèrement dégradé (3 tampons en retard, max 15,7 ms) et
   **un run franchement décroché** (255 tampons en retard, lateness max **134,4 ms**,
   marge **−111,9 ms**, ratio 0,959) — le thread non priorisé a été désordonnancé par la
   charge.
2. **Au repos, MMCSS n'apporte rien de mesurable** : les deux configurations tiennent le
   temps réel (0 tampon en retard) ; la lateness max observée est même un peu plus haute
   avec MMCSS (7,7–8,3 ms) que sans (1,8–6,1 ms sur 3 runs) alors que le callback max est
   plus bas avec MMCSS (464–478 µs vs 890–1 853 µs). L'écart est dans la variabilité
   run-à-run : sur une machine non saturée, MMCSS ne peut pas « améliorer » ce qui n'est
   pas dégradé.
3. **MMCSS n'est pas du hard-RT.** « Pro Audio » + `AVRT_PRIORITY_CRITICAL` est une
   demande de classe d'ordonnancement ; le système ne garantit aucune échéance. Ça ne
   corrige ni le pilote `waveOut` legacy, ni la quantification de livraison des
   `WOM_DONE`, ni les limites du chemin audio mesurées au §4.
4. **La charge est artificielle** : 12 boucles PowerShell occupent le CPU, mais elles ne
   produisent ni interruptions de pilote audio, ni accès disque/PCIe comparables à un
   DAW. Le résultat dit « MMCSS protège ce thread contre une charge CPU concurrente »,
   pas « MMCSS garantit l'audio en production ».
5. **Deux répétitions par cellule** (plus une 3e exécution de la cellule repos sans MMCSS
   pour la reproductibilité, cohérente : ratio 1,0006, 0 tampon en retard, lateness max
   6 139 µs, marge +25,5 ms) : l'effet est cohérent avec le mécanisme et net, mais ce
   n'est pas une preuve statistique. Les cellules ont été exécutées dans un ordre fixe
   (repos sans, repos avec, charge sans, charge avec), pas en ordre aléatoire, et la
   machine est partagée avec d'autres agents : une partie de la variabilité vient de là.
   Toujours pas de WASAPI/ASIO, pas de XRUN mesurable, pas de fenêtre longue.

Preuves : 9 rapports bruts
`evidence/mvp0b-audio-mmsc-{rest-nommscss-r1,rest-nommscss-r2,rest-nommscss-r3,rest-mmcss-r1,rest-mmcss-r2,load-nommscss-r1,load-nommscss-r2,load-mmcss-r1,load-mmcss-r2}-2026-11-08.json`,
plus le récapitulatif **dérivé** `evidence/mvp0b-audio-mmsc-campaign-2026-11-08.json`
(celui-ci n'est pas une mesure : il agrège les rapports bruts et le dit).

## 6. WASAPI mode partagé (backend `--api wasapi`)

### 6.1 Ce qui est implémenté

La sonde accepte `--api waveout|wasapi` (défaut `waveout`, pour ne pas casser les
campagnes existantes). Le backend `wasapi` n'ajoute **aucune dépendance externe** : FFI
directe de `ole32`/`mmdevapi`/`kernel32`, en `extern "system"`.

- COM (`CoInitializeEx` MTA) sur le **thread de rendu**, `MMDeviceEnumerator` →
  endpoint de rendu par défaut, `IMMDevice::OpenPropertyStore` pour le nom convivial et
  `IMMDevice::GetId` pour l'identifiant stable de l'endpoint ;
- `IAudioClient::GetMixFormat` : format du **moteur audio** (et non un format choisi par
  l'application), `IAudioClient::GetDevicePeriod` (période par défaut et minimale) ;
- `Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, duration, 0,
  mix_format, NULL)` avec `duration` dérivé de `--frames`, puis `SetEventHandle` ;
- boucle événementielle : `WaitForSingleObject`, `GetCurrentPadding`, si la place le
  permet `GetBuffer` → écriture de **zéros** → `ReleaseBuffer` ; le signal DSP est calculé
  par `BlockHost::process_rt`, mesuré (travail, delta inter-échantillon), puis jeté : le
  endpoint ne reçoit **que du silence** ;
- swaps à chaud du module réel via les vraies portes (`prepare`/preuve/`apply`) et crossfade
  optionnel, exactement comme pour `waveOut` ;
- `--mmcss` / `--thread-priority` réutilisés sur le thread de rendu ;
- arrêt propre : `Stop`, release de toutes les interfaces COM, `CoUninitialize`,
  `AvRevertMmThreadCharacteristics` ; toute erreur est un échec explicite avec HRESULT.

**Définition de la starvation** (la plus proche observation possible d'un underrun) : un
événement où `GetCurrentPadding == bufferFrames` au moment où l'application veut écrire —
le buffer du endpoint est plein, aucune frame ne peut être soumise. Un XRUN réel n'est
**pas** exposé par WASAPI en mode partagé.

### 6.2 Périphérique, format et période observés

| Élément | Valeur observée |
|---|---|
| Endpoint par défaut | `Out 1-2 (MOTU M Series)` |
| Format du moteur (mix format) | **float32**, 2 canaux, **48 000 Hz**, block align 8 |
| Buffer du endpoint | **1 056 frames = 22 000 µs** (`buffer_duration_us`) |
| Période du device | **10 000 µs** par défaut (**3 000 µs** minimum annoncée) |
| Mode | partagé, événementiel (`EVENTCALLBACK`), événements à ~100 Hz |
| Coût d'activation du moteur | `open_prepare_us` 0,28–0,55 s (mesuré séparément, hors période) |

Le buffer du endpoint (22 ms) est plus grand que la période du device (10 ms) : la sonde
écrit des blocs de 480 à 960 frames à chaque événement, et « place disponible < buffer »
est **normal** (276 événements sur un run de 3 s) : ce n'est pas une faute.

### 6.3 Résultats réels : 4 cellules × 2 runs (même protocole, 3 s, module + swaps)

| Cellule | CPU mesuré | cadence (ratio) | lateness p50 | p95 | max | événements en retard >1 période | starvations | callback p95 | work p95 | alloc/free | swaps |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|
| repos sans MMCSS r1 | 24,2 % | 1,0001 | −121 µs | 598 µs | 1 077 µs | 0 | 18 | 50 µs | 16 µs | 0/0 | 11/11 |
| repos sans MMCSS r2 | 39,8 % | 1,0005 | −367 µs | 194 µs | 703 µs | 0 | 0 | 54 µs | 14 µs | 0/0 | 11/11 |
| repos MMCSS r1 | 44,5 % | 1,0009 | −216 µs | 377 µs | 841 µs | 0 | 0 | 50 µs | 15 µs | 0/0 | 11/11 |
| repos MMCSS r2 | 29,7 % | 1,0007 | 174 µs | 847 µs | 1 198 µs | 0 | 18 | 46 µs | 13 µs | 0/0 | 11/11 |
| **charge 100 % sans MMCSS r1** | 99,0 % | 1,0003 | −314 µs | 335 µs | 716 µs | 0 | 4 | 61 µs | 14 µs | 0/0 | 11/11 |
| **charge 100 % sans MMCSS r2** | 100,0 % | 1,0006 | −22 µs | 485 µs | 1 398 µs | 0 | 0 | 56 µs | 14 µs | 0/0 | 11/11 |
| **charge 100 % MMCSS r1** | 99,5 % | 1,0005 | −267 µs | 180 µs | 694 µs | 0 | 0 | 58 µs | 14 µs | 0/0 | 11/11 |
| **charge 100 % MMCSS r2** | 99,8 % | 1,0007 | 319 µs | 933 µs | 1 242 µs | 0 | 18 | 62 µs | 16 µs | 0/0 | 11/11 |

Sur les 8 runs : cadence tenue (ratio 1,0001–1,0009), **0 événement en retard de plus
d'une période**, lateness max **0,7–1,4 ms**, travail DSP p50 5–8 µs / p95 10–16 µs,
callback complet p95 46–62 µs, **0 allocation et 0 désallocation sur le thread de rendu**,
**11/11 swaps activés**, `|delta|` aux swaps 0,0290–0,0293 avec fondu. Les starvations
observées (0, 4 ou 18 selon le run) sont des moments où le buffer de 22 ms était plein :
elles n'ont produit ni retard > période ni échantillon manquant mesurable ; la définition
est conservée telle quelle car WASAPI partagé n'expose pas de compteur d'XRUN.

### 6.4 Comparaison honnête avec `waveOut` (mêmes 3 s, même module, même machine)

| Chemin | Buffer | Sous charge 100 %, sans MMCSS | Sous charge 100 %, avec MMCSS |
|---|---|---|---|
| `waveOut` (legacy) | 512 frames, 10,67 ms de période, 4 tampons | 1 run dégradé (3 tampons en retard, max 15,7 ms) et **1 run décroché (255 tampons, max 134,4 ms, marge −111,9 ms)** | **0 tampon en retard**, max 2,5–2,7 ms |
| **WASAPI partagé** | endpoint 1 056 frames, 22 ms ; période device 10 ms | **0 événement en retard**, lateness max 0,7–1,4 ms, starvations 0/4 | **0 événement en retard**, max 0,7–1,2 ms, starvations 0/18 |

Lecture : le chemin événementiel WASAPI moderne est **beaucoup plus robuste à une charge
CPU concurrente** que `waveOut` legacy, et MMCSS n'y apporte pas de gain visible (les deux
configurations tiennent) — à l'inverse de `waveOut`, où MMCSS était décisif sous charge.
Deux réserves : (a) les métriques ne sont pas identiques (`waveOut` compte des
soumissions de tampon en retard, WASAPI compte des réveils tardifs et des starvations),
même si les deux capturent un retard de livraison ; (b) le mode partagé impose le buffer
du moteur (22 ms ici), donc une latence de sortie plus grande que les 10,67 ms de
`waveOut` — ce n'est pas une qualification de latence audio, seulement un constat.
**Aucune revendication hard-RT** : pas de MMCSS obligatoire pour WASAPI ici, pas de XRUN
mesuré, un seul endpoint et un seul OS.

### 6.5 Mode exclusif WASAPI (`--api wasapi-exclusive`)

#### 6.5.1 Ce qui est implémenté

`--api wasapi-exclusive` (ou `--api wasapi --exclusive`) ouvre un flux **exclusif** sur
un endpoint de rendu : plus aucun mélange du moteur audio, l'application possède le
**format** et la **période**. FFI directe, aucune dépendance ajoutée.

- Le format est un **PCM 16 bits, 2 canaux, 48 kHz** (par défaut) construit par la sonde
  en `WAVEFORMATEXTENSIBLE` ; `--exclusive-rate` et `--exclusive-bits` (16/24/32)
  permettent de balayer d'autres combinaisons.
- Avant toute ouverture, `IAudioClient::IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE,
  format, &closest)` est appelé sur **12 combinaisons** : 2 layouts
  (`WAVEFORMATEXTENSIBLE` et `WAVEFORMATEX` classique) × 2 taux (taux du mix, 44 100 Hz,
  48 000 Hz) × 3 profondeurs (16/24/32 bits). Le JSON `exclusive.format_sweep` contient
  chaque combinaison avec son HRESULT et son statut (`accepted` / `closest_match_only` /
  `refused`).
- Si la combinaison demandée est refusée, la sonde **échoue explicitement** : statut
  `refused`, code HRESULT exact, format essayé, balayage complet et support exclusif de
  chaque endpoint ; **aucune mesure n'est inventée**.
- `Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, EVENTCALLBACK, duration, duration, format,
  NULL)` (en mode exclusif la périodicité est égale à la durée du buffer), puis
  `SetEventHandle`, `GetBufferSize`, `GetStreamLatency`, `GetService(IAudioRenderClient)`,
  **pré-roll d'un buffer de silence**, puis `Start`.
- Boucle identique au partagé : `WaitForSingleObject`, `GetCurrentPadding` (enregistré
  pour diagnostic), `GetBuffer` → **zéros uniquement** → `ReleaseBuffer`, travail DSP
  mesuré puis jeté. En exclusif, la sonde écrit **un buffer complet par événement** (le
  buffer applicatif est la période) ; `GetCurrentPadding` reste épinglé à `bufferFrames`
  sur ce pilote et n'est donc pas utilisé pour dimensionner l'écriture.
- `--mmcss` / `--thread-priority`, swaps à chaud + crossfade, mesures (lateness, work,
  callback, starvations, alloc/free, ratio, swaps) : mêmes mécanismes que le partagé.
- Arrêt propre : `Stop`, release des interfaces, `CoUninitialize`, revert MMCSS.
- `--exclusive-endpoint N` ouvre l'endpoint de rendu actif N au lieu du périphérique par
  défaut, **sans modifier le périphérique par défaut du système** (diagnostic) ; l'ordre
  est celui de `IMMDeviceEnumerator::EnumAudioEndpoints` et le JSON reporte l'`endpoint_id`
  stable + le support exclusif de chaque endpoint.

#### 6.5.2 Formats acceptés / refusés (résultat réel)

Le **périphérique par défaut (MOTU, WDM)** refuse **les 12 combinaisons** testées :
`AUDCLNT_E_UNSUPPORTED_FORMAT` (HRESULT `0x88890008`, soit −2004287480), aussi bien en
`WAVEFORMATEXTENSIBLE` qu'en `WAVEFORMATEX` classique, à 44,1 kHz comme à 48 kHz, en
16/24/32 bits. C'est un **résultat négatif explicite**, pas un défaut du harnais —
preuve `evidence/mvp0b-audio-wasapi-exclusive-default-refused-2026-11-08.json`.

Un balayage **read-only** (`IsFormatSupported` seul, aucun flux ouvert) sur tous les
endpoints de rendu actifs explique le refus :

| Endpoint | Support exclusif PCM16 (4 essais : mix/44,1/48 kHz × 2 layouts) |
|---|---:|
| `Haut-parleurs (Realtek(R) Audio)` | **4/4 acceptés** |
| `FHD (NVIDIA High Definition Audio)` (index 1) | **4/4 acceptés** |
| `FHD (NVIDIA High Definition Audio)` (index 2) | 2/4 acceptés |
| `Out 1-2 (MOTU M Series)` — défaut | **0/4 accepté** |

Le mode exclusif est donc disponible sur cette machine, mais **pas sur l'interface
MOTU par défaut**. Une tentative réelle sur l'endpoint Realtek (Initialize accepté) n'a
pas produit un flux stable : 76 timeouts sur 2 s, ratio 0,975, lateness p50 15,7 ms /
p95 53 ms — probablement parce qu'aucune sortie physique n'est active sur cet endpoint ;
elle n'a pas été retenue comme plateforme de mesure.

La campagne a donc été exécutée sur l'endpoint **NVIDIA HDMI (index 1)**, format
**PCM16 extensible 48 kHz 2 canaux** :

| Élément | Valeur observée |
|---|---|
| Endpoint | `FHD (NVIDIA High Definition Audio)`, index actif 1 (`endpoint_id` dans le JSON) |
| Format exclusif | **pcm_s16le**, `WAVE_FORMAT_EXTENSIBLE`, 2 canaux, **48 000 Hz**, block align 4, layout `extensible` |
| Buffer du endpoint | **480 frames = 10 000 µs** (`GetBufferSize`) |
| Période | **10 000 µs**, possédée par l'application (`hnsBufferDuration == hnsPeriodicity`) |
| Latence de flux | `GetStreamLatency` = **10 000 µs** |
| Périodes device | 10 000 µs par défaut, 3 000 µs minimum annoncée |
| Durée de buffer demandée | 480 frames à 48 kHz = 10 000 µs (non arrondie, pas de retry d'alignement) |

#### 6.5.3 Résultats réels (5 runs, 3 s, module + swaps + crossfade 256, sans MMCSS)

| Cellule | CPU mesuré | ratio | work p50/p95/max (µs) | callback p50/p95/max (µs) | lateness p50/p95/max (µs) | tardifs > 1 période | starv. | alloc/free | swaps | `\|delta\|` aux swaps |
|---|---:|---:|---|---|---|---:|---:|---|---:|---:|
| repos r1 | 26,4 % | 1,0000 | 7/14/99 | 65/170/361 | 10/161/558 | 0 | 0 | 0/0 | 11/11 | 0,0293 |
| repos r2 | 32,2 % | 1,0000 | 7/15/104 | 75/172/1 133 | 88/256/705 | 0 | 0 | 0/0 | 11/11 | 0,0293 |
| repos r3 (reprise) | 27,2 % | 1,0000 | 7/11/281 | 66/172/453 | 32/139/250 | 0 | 0 | 0/0 | 11/11 | 0,0293 |
| **charge 100 % r1** | 100,0 % | 1,0000 | 11/47/511 | 113/461/1 176 | −241/1 196/3 498 | **0** | 0 | 0/0 | 11/11 | 0,0291 |
| **charge 100 % r2** | 99,6 % | 1,0000 | 11/25/148 | 117/493/1 105 | −160/967/3 476 | **0** | 0 | 0/0 | 11/11 | 0,0293 |

Référence sinusoïdale du delta : 0,0288 ; hors swap : 0,0287 ; aucun échantillon
non fini. Événements traités : 294 par run (302 avec warm-up), soit exactement la
cadence attendue de 100 Hz sur 3 s.

#### 6.5.4 Lecture honnête

1. **Résultat négatif important** : sur cette machine, le périphérique audio principal
   (MOTU M Series, endpoint par défaut) **n'expose pas le mode exclusif WASAPI** — le
   pilote WDM refuse les 12 formats balayés. Pour utiliser l'interface MOTU en latence
   minimale, il faudrait passer par **ASIO**, non implémenté ici. Ce n'est pas une
   conclusion sur « WASAPI exclusif » en général : d'autres endpoints de la même machine
   l'acceptent.
2. **Le mode exclusif mesuré** (NVIDIA HDMI, PCM16 48 kHz, période applicative 10 ms)
   tient la cadence (ratio 1,0000 sur 5 runs), avec **0 événement en retard de plus d'une
   période, 0 starvation, 0 allocation/désallocation** sur le thread de rendu, et 11/11
   swaps à chaque run. Au repos, lateness max 0,25–0,71 ms ; sous charge CPU 100 % (sans
   MMCSS), lateness p95 0,97–1,20 ms / max 3,5 ms, callback p95 0,46–0,49 ms, travail DSP
   p95 25–47 µs.
3. **Comparaison avec le mode partagé (§6.4)** : les deux modes tiennent le temps réel
   avec 0 événement en retard. Le mode exclusif offre une **période de sortie de 10 ms**
   (buffer 480 frames) contre **22 ms** en partagé (buffer moteur 1 056 frames), donc moins
   de latence de sortie mais aussi **moins de marge** : sous charge, la lateness max monte
   à 3,5 ms en exclusif contre 0,7–1,4 ms en partagé, tout en restant sous la période.
   **Réserve de méthode** : la campagne partagée utilisait l'endpoint MOTU par défaut, la
   campagne exclusive l'endpoint NVIDIA ; le mode et l'endpoint varient donc ensemble — on
   ne peut pas isoler l'effet du seul mode.
4. **Pas de hard-RT, pas d'XRUN mesuré** : un XRUN réel reste non observable de façon
   fiable ; les starvations définies (§6.1) et les événements en retard sont les signaux
   proxy. 5 runs, un seul OS, un seul pilote HDMI, fenêtres de 3 s ; aucune extrapolation.

## 7. Ce qui n'a pas été exécuté (et pourquoi)

| Élément du mandat L4-C | Statut | Raison |
|---|---|---|
| Test dans un hôte audio réel (CLAP/VST3/DAW) | **non exécuté** | Ravum ne produit pas encore de binaire plugin CLAP/VST3 ; en créer un est un travail d'ABI distinct (hôte, GUI, paramètres, bus) hors périmètre de ce lot. |
| Périphérique réel via **WASAPI mode partagé** | **exécuté** (§6) : endpoint par défaut, mix format float32 48 kHz, buffer 22 ms, événementiel, 8 runs (repos/charge × MMCSS), 11/11 swaps, 0 alloc/free | — |
| Périphérique réel via **WASAPI mode exclusif** | **partiellement exécuté** (§6.5) : refus explicite (`AUDCLNT_E_UNSUPPORTED_FORMAT`) du périphérique par défaut MOTU pour les 12 formats balayés ; mesure réelle sur l'endpoint NVIDIA HDMI (PCM16 48 kHz, période 10 ms, 5 runs, 0 événement en retard, 11/11 swaps). Aucune mesure exclusive sur le MOTU ; le périphérique par défaut du système n'a pas été changé. | — |
| Périphérique réel via **ASIO** | **non exécuté** | ASIO exige un SDK tiers et un pilote ASIO dédié ; lot suivant. C'est pourtant le chemin nécessaire pour utiliser l'interface MOTU en latence minimale, puisque son endpoint WDM refuse l'exclusif. |
| Identification OS / pilote / sample rate / buffer size | **partiel** : OS et périphériques identifiés (§2), format/période/taille de tampon et quantum de service observés (§4, §6, §6.5), mais le pilote interne et l'endpoint physique résolu par `WAVE_MAPPER` restent inconnus (`waveOutGetID` renvoie `0xFFFFFFFF`). | — |
| Mesure des XRUN | **toujours non exécutée** | Un XRUN est un événement de pilote ; ni waveOut ni WASAPI partagé ne l''exposent. On mesure des tampons soumis en retard, des réveils tardifs, une marge de file dérivée et des **starvations définies** (buffer plein au moment d''écrire), nommés comme tels. |
| Durée de callback sur périphérique réel | **mesurée** (§4.3) : travail `process_rt` p50 7 µs / p95 14 µs / max 83 µs ; callback complet (réveil → re-soumission) p50 275 µs / p95 503 µs / max 756 µs | — |
| Swaps répétés dans un callback réel | **mesurés** (§4.3, §4.5) : 11 activations par run sous pilote, 0 refus, avec et sans fondu | — |
| Continuation d'état à travers un swap en conditions réelles | **mesurée** (§4.5) : `\|delta\|` aux swaps 0,0291 avec fondu (0,1260 sans), état possédé par l'hôte | — |
| Reproductibilité des erreurs audio (dropouts, glitches) | **partielle** : plusieurs runs de référence, un run sous charge contrôlée (§4.6) et la campagne MMCSS 4 cellules × 2 runs + 1 reprise (§5) ; pas de fenêtre longue (minutes), pas de compteur de glitches du pilote. | — |
| Priorité temps réel MMCSS | **mesurée** (§5) : `AvSetMmThreadCharacteristicsW("Pro Audio")` + `AVRT_PRIORITY_CRITICAL`, priorité de base observée 18, revert ok ; 2 runs/cellule sous charge CPU 100 % | — |
| Priorité Win32 brute (`SetThreadPriority`) | **implémentée et vérifiée** : `normal`/`above`/`high`/`time-critical` (priorités observées 0/1/2/15), appliquée après MMCSS lorsqu'elles sont demandées ensemble | — |
| WASAPI exclusive / ASIO / priorité temps réel WASAPI | **exclusif partiellement exécuté, ASIO non** | Le mode exclusif est implémenté et mesuré sur l'endpoint NVIDIA HDMI (§6.5) ; le périphérique par défaut MOTU le refuse explicitement. ASIO reste à faire. MMCSS ne compense pas : c'est une classe d'ordonnancement, pas un pilote certifié. |
| Qualification hard-réaliste | **non exécutée et non revendiquée** | Voir §4.7 et §5.4 : la voie utilisée n'est pas certifiée, MMCSS ne promet pas d'échéance et la charge machine fait varier le résultat. |

Ces éléments restent des **travaux non faits**, et aucune conclusion de qualité audio
réelle ne doit en être tirée.

## 8. Plan proposé pour une vraie qualification (partiellement commencé)

1. ~~**Option la moins risquée** : boucle de rendu silencieuse qui appelle `process_rt`
   dans le callback du pilote.~~ **Fait avec `waveOut`** (§4) : la boucle existe, ne fait
   aucun son, et mesure travail/lateness/swaps/allocations sur un vrai périphérique.
   **Fait aussi : MMCSS** (§5), la priorité haute est mesurée sous charge. **Fait aussi :
   WASAPI partagé (§6) puis exclusif (§6.5)**. Reste à faire la version **WASAPI sur le
   périphérique par défaut** (refusée par le pilote MOTU, à contourner par ASIO) et le
   chemin **ASIO** lui-même.
2. **Option produit** : construire un plugin CLAP minimal autour de l'ABI de bloc
   (`clap_entry`, process, params) et l'héberger dans un hôte réel (Seno/Reaper/…). Exige
   un ABI hôte distinct et une GUI ; lot séparé.
3. Définir explicitement la plateforme qualifiée (OS, pilote, sample rate, buffer size,
   priorité du thread) et **ne jamais** extrapoler le résultat d'un desktop à une garantie
   hard-RT.
4. Mesurer les XRUN sur une fenêtre longue (minutes) avec une API qui les expose
   (WASAPI : `IAudioClient`/événements de glitch ou compteurs du pilote), publier les
   distributions, pas seulement le maximum.
5. Ne pas figer 256 frames comme taille par défaut sur cette machine : la voie par défaut
   ne tient pas le temps réel à 5,33 ms (§4.4) ; 512 frames (10,67 ms) est la plus petite
   taille mesurée qui tient.

## 9. Reproduire

```powershell
cd D:\App\Ravum

# 1) Mesure réelle sur périphérique (compile iir_v1/iir_v2, lance la sonde, écrit la preuve).
#    Le périphérique ne reçoit que du silence ; le signal DSP est calculé puis jeté.
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1

# Variantes mesurées dans ce document :
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Frames 256          # réglage par défaut -> ratio 0,53 sur le périphérique par défaut
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Device 1 -Frames 256 # Realtek direct -> ratio 1,001
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -CrossfadeFrames 0    # clic attendu (delta 0,1260)
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -ListDevices          # indices waveOut

# 2) Campagne MMCSS : repos / charge × sans / avec MMCSS, 2 répétitions chacune.
#    -Mmcss enregistre le thread de rendu "Pro Audio" + AVRT_PRIORITY_CRITICAL ;
#    -Load 12 crée 12 processus PowerShell busy-loop, bornés et arrêtés en fin de run ;
#    -Variant nomme la preuve evidence/mvp0b-audio-mmsc-<variant>-<date>.json.
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Variant rest-nommscss-r1
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Variant rest-nommscss-r2
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Mmcss -Variant rest-mmcss-r1
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Mmcss -Variant rest-mmcss-r2
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Load 12 -Variant load-nommscss-r1
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Load 12 -Variant load-nommscss-r2
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Mmcss -Load 12 -Variant load-mmcss-r1
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Mmcss -Load 12 -Variant load-mmcss-r2

# Une seule cellule avec MMCSS + priorité Win32 brute (séparation des deux mécanismes) :
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Mmcss -ThreadPriority time-critical -Variant smoke-mmcss-tc

# 2bis) Backend WASAPI mode partage (silence, evenementiel) : 4 cellules de la campagne.
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Api wasapi -Variant rest-nommscss-r1
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Api wasapi -Mmcss -Variant rest-mmcss-r1
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Api wasapi -Load 12 -Variant load-nommscss-r1
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Api wasapi -Mmcss -Load 12 -Variant load-mmcss-r1

# 2ter) Backend WASAPI mode exclusif (silence). Le peripherique par defaut MOTU refuse
#    tous les formats exclusifs : la preuve de refus s'obtient sans -ExclusiveEndpoint.
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Api wasapi-exclusive -Out evidence/mvp0b-audio-wasapi-exclusive-default-refused-2026-11-08.json
#    Mesures exclusives reelles sur l'endpoint NVIDIA HDMI (index actif 1), sans changer
#    le peripherique par defaut du systeme :
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Api wasapi-exclusive -ExclusiveEndpoint 1 -Frames 480 -Variant rest-r1
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Api wasapi-exclusive -ExclusiveEndpoint 1 -Frames 480 -Variant rest-r2
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Api wasapi-exclusive -ExclusiveEndpoint 1 -Frames 480 -Load 12 -Variant load-r1
powershell -ExecutionPolicy Bypass -File scripts/audio-probe.ps1 -Api wasapi-exclusive -ExclusiveEndpoint 1 -Frames 480 -Load 12 -Variant load-r2

# Commande directe de la sonde (module précompilé par ravum build) :
cargo run -q -p ravum-dsp-harness --bin ravum-audio-probe -- --seconds 3 --frames 512 `
  --module target/ravum/artifacts/<iir_v1>.dll `
  --swap-build target/ravum/audio-probe/build-iir_v2.json `
  --swap-build target/ravum/audio-probe/build-iir_v1.json `
  --swap-every-ms 250 --crossfade-frames 256 --json --out evidence/mvp0b-audio-probe-2026-11-08.json

# 3) Gigue d'ordonnanceur OS (inchangée) :
cargo run -q -p ravum-dsp-harness --bin ravum-jitter -- --iterations 2000 --period-us 5333 --json

# 4) Simulateur de blocs (avec crossfade) :
cargo run -q --bin ravum -- dsp fixtures/dsp/iir_v1.rs --next fixtures/dsp/iir_v2.rs --blocks 400 --crossfade-frames 256 --json
```

Preuves JSON de ce document :

| Fichier | Configuration | Résultat |
|---|---|---|
| `evidence/mvp0b-audio-probe-2026-11-08.json` | défaut, 512 frames, 3 s, iir_v1→v2, fondu 256 | ratio 1,001, 0 alloc, 11 swaps, delta swaps 0,0291 |
| `evidence/mvp0b-audio-probe-2026-11-08-f256.json` | défaut, 256 frames | ratio 0,53, 292 tampons en retard, file vidée |
| `evidence/mvp0b-audio-probe-2026-11-08-realtek-f256.json` | index 1 (Realtek), 256 frames | ratio 1,001, 0 risque de vidage |
| `evidence/mvp0b-audio-probe-2026-11-08-nocrossfade.json` | défaut, 512 frames, sans fondu | delta swaps 0,1260 (clic) |
| `evidence/mvp0b-audio-probe-2026-11-08-under-load.json` | défaut, 512 frames, charge CPU 100 % | lateness max 207 862 µs, 240 tampons en retard |
| `evidence/mvp0b-audio-mmsc-rest-nommscss-r{1,2}-2026-11-08.json` | repos, sans MMCSS | 0 tampon en retard, lateness max 1 785 / 4 988 µs |
| `evidence/mvp0b-audio-mmsc-rest-nommscss-r3-2026-11-08.json` | repos, sans MMCSS, reproductibilité | 0 tampon en retard, lateness max 6 139 µs |
| `evidence/mvp0b-audio-mmsc-rest-mmcss-r{1,2}-2026-11-08.json` | repos, MMCSS Pro Audio + critical | 0 tampon en retard, lateness max 7 693 / 8 307 µs |
| `evidence/mvp0b-audio-mmsc-load-nommscss-r{1,2}-2026-11-08.json` | charge 100 %, sans MMCSS | 3 / 255 tampons en retard, lateness max 15 682 / 134 445 µs |
| `evidence/mvp0b-audio-mmsc-load-mmcss-r{1,2}-2026-11-08.json` | charge 100 %, MMCSS | **0 / 0** tampon en retard, lateness max 2 538 / 2 700 µs |
| `evidence/mvp0b-audio-mmsc-campaign-2026-11-08.json` | récapitulatif **dérivé** des 9 runs bruts (pas une mesure) | agrégats par cellule |
| `evidence/mvp0b-audio-wasapi-exclusive-default-refused-2026-11-08.json` | exclusif sur le périphérique par défaut MOTU | **refusé** : `AUDCLNT_E_UNSUPPORTED_FORMAT` sur 12/12 formats, endpoint_support 4 endpoints |
| `evidence/mvp0b-audio-wasapi-exclusive-rest-r{1,2,3}-2026-11-08.json` | exclusif NVIDIA HDMI, repos | ratio 1,0000, 0 événement en retard, lateness max 0,25–0,71 ms, 0 alloc, 11/11 swaps |
| `evidence/mvp0b-audio-wasapi-exclusive-load-r{1,2}-2026-11-08.json` | exclusif NVIDIA HDMI, charge CPU 100 % | ratio 1,0000, 0 événement en retard, lateness max 3,5 ms, 0 alloc, 11/11 swaps |
| `evidence/mvp0b-audio-wasapi-exclusive-campaign-2026-11-08.json` | récapitulatif **dérivé** des 5 runs + refus (pas une mesure) | agrégats + support exclusif par endpoint |
