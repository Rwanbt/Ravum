# MVP-0b / L4-C — Chemin audio réel : ce qui est mesuré et ce qui ne l'est pas

> **Ce document existe pour éviter une fausse revendication.** Le démonstrateur DSP de
> Ravum est un **simulateur de blocs**, et une **sonde réelle** exécute désormais le vrai
> chemin DSP sur un périphérique `waveOut` Windows **en n'émettant que du silence**. Ce
> document rapporte (1) l'inventaire matériel réel de la machine, (2) une mesure réelle
> d'ordonnanceur OS qui borne honnêtement les prétentions, (3) les mesures réelles sur
> périphérique (travail DSP, latence, swaps, allocations), (4) la liste explicite de ce
> qui n'a pas été exécuté et pourquoi.
>
> **Ce qui n'est toujours pas qualifié** : pas de WASAPI/ASIO, pas de priorité temps réel
> ni de MMCSS, pas d'hôte CLAP/VST3/DAW, pas de XRUN mesurable via `waveOut`, **aucune**
> qualification hard-réaliste. Une sonde `waveOut` legacy n'est pas un pilote audio certifié.

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
| Périphérique réel via WASAPI/ASIO, priorité RT, MMCSS | **aucun** |
| Hôte CLAP/VST3/ASIO branché sur le module | **aucun** |
| XRUN mesurés, glitches de pilote comptés | **aucun** (`waveOut` ne les expose pas) |

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
d'autres agents de la machine compilaient. C'est exactement la raison pour laquelle aucune
revendication hard-RT n'est faite.

### 4.7 Ce que ces chiffres ne prouvent pas

- `waveOut` (winmm) est une API **legacy émulée**, pas un pilote temps réel certifié :
  pas de WASAPI exclusive, pas d'ASIO, pas d'enregistrement MMCSS, pas de priorité
  `THREAD_PRIORITY_TIME_CRITICAL`. Ce n'est **pas** une qualification hard-réaliste.
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
  la variabilité run-à-run est réelle et documentée (§4.3, §4.6).

## 5. Ce qui n'a pas été exécuté (et pourquoi)

| Élément du mandat L4-C | Statut | Raison |
|---|---|---|
| Test dans un hôte audio réel (CLAP/VST3/DAW) | **non exécuté** | Ravum ne produit pas encore de binaire plugin CLAP/VST3 ; en créer un est un travail d'ABI distinct (hôte, GUI, paramètres, bus) hors périmètre de ce lot. |
| Périphérique réel via WASAPI/ASIO | **non exécuté** — seul `waveOut` legacy a été utilisé | WASAPI/ASIO exigent une boucle COM/WASAPI (ou SDK ASIO) et une sortie physique ; lot suivant. Une exécution réelle aurait des effets audibles non sollicités ; la sonde `waveOut` contourne ce point en n'émettant que du silence. |
| Identification OS / pilote / sample rate / buffer size | **partiel** : OS et périphériques identifiés (§2), format/période/taille de tampon et quantum de service observés (§4), mais le pilote interne et l'endpoint physique résolu par `WAVE_MAPPER` restent inconnus (`waveOutGetID` renvoie `0xFFFFFFFF`). | — |
| Mesure des XRUN | **toujours non exécutée** | Un XRUN est un événement de pilote ; `waveOut` ne l'expose pas. On mesure des tampons soumis en retard et une marge de file dérivée, nommés comme tels. |
| Durée de callback sur périphérique réel | **mesurée** (§4.3) : travail `process_rt` p50 7 µs / p95 14 µs / max 83 µs ; callback complet (réveil → re-soumission) p50 275 µs / p95 503 µs / max 756 µs | — |
| Swaps répétés dans un callback réel | **mesurés** (§4.3, §4.5) : 11 activations par run sous pilote, 0 refus, avec et sans fondu | — |
| Continuation d'état à travers un swap en conditions réelles | **mesurée** (§4.5) : `\|delta\|` aux swaps 0,0291 avec fondu (0,1260 sans), état possédé par l'hôte | — |
| Reproductibilité des erreurs audio (dropouts, glitches) | **partielle** : plusieurs runs de référence et un run sous charge contrôlée (§4.6) ; pas de fenêtre longue (minutes), pas de compteur de glitches du pilote. | — |
| Priorité temps réel / MMCSS / WASAPI exclusive | **non exécuté** | `waveOut` n'offre pas ce chemin ; lot suivant. |
| Qualification hard-réaliste | **non exécutée et non revendiquée** | Voir §4.7 : la voie utilisée n'est pas certifiée et la charge machine fait varier le résultat. |

Ces éléments restent des **travaux non faits**, et aucune conclusion de qualité audio
réelle ne doit en être tirée.

## 6. Plan proposé pour une vraie qualification (partiellement commencé)

1. ~~**Option la moins risquée** : boucle de rendu silencieuse qui appelle `process_rt`
   dans le callback du pilote.~~ **Fait avec `waveOut`** (§4) : la boucle existe, ne fait
   aucun son, et mesure travail/lateness/swaps/allocations sur un vrai périphérique.
   Reste à faire la version **WASAPI** *shared* puis *exclusive* (FFI directe ou crate
   `windows`) avec `AvSetMmThreadCharacteristics` (MMCSS) et priorité de thread, qui
   permettra de comparer `waveOut` legacy à un chemin moderne.
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

## 7. Reproduire

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

# Commande directe de la sonde (module précompilé par ravum build) :
cargo run -q -p ravum-dsp-harness --bin ravum-audio-probe -- --seconds 3 --frames 512 `
  --module target/ravum/artifacts/<iir_v1>.dll `
  --swap-build target/ravum/audio-probe/build-iir_v2.json `
  --swap-build target/ravum/audio-probe/build-iir_v1.json `
  --swap-every-ms 250 --crossfade-frames 256 --json --out evidence/mvp0b-audio-probe-2026-11-08.json

# 2) Gigue d'ordonnanceur OS (inchangée) :
cargo run -q -p ravum-dsp-harness --bin ravum-jitter -- --iterations 2000 --period-us 5333 --json

# 3) Simulateur de blocs (avec crossfade) :
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
