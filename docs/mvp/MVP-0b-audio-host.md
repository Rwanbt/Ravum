# MVP-0b / L4-C — Chemin audio réel : ce qui est mesuré et ce qui ne l'est pas

> **Ce document existe pour éviter une fausse revendication.** Le démonstrateur DSP de
> Ravum est un **simulateur de blocs** : aucun périphérique audio, aucun pilote, aucune
> priorité temps réel, aucun XRUN. Ce document rapporte (1) l'inventaire matériel réel de
> la machine, (2) une mesure réelle d'ordonnancement OS qui borne honnêtement les
> prétentions, (3) la liste explicite de ce qui n'a pas été exécuté et pourquoi.

## 1. Ce qui est réellement implémenté et mesuré aujourd'hui

| Élément | Statut |
|---|---|
| ABI de bloc v2 (`ravum_process_block`), état possédé par l'hôte | implémenté, testé |
| Publication sans verrou, garde non possédante sur le callback | implémenté, testé (`MVP-0b-rt-lifecycle.md`) |
| 0 allocation / 0 désallocation sur le thread « audio » (simulé) | mesuré sur 199 517 blocs |
| Crossfade de remplacement (clics M3 0,108/0,657 → 0,029/0,058) | implémenté, mesuré (`MVP-0b-dsp-crossfade.md`) |
| Cadence simulée `frames / sample_rate` | mesuré (période dérivée 5 333 µs) |
| Périphérique audio, pilote, tampon, XRUN, priorité RT | **aucun** |
| Hôte CLAP/VST3/ASIO/WASAPI branché sur le module | **aucun** |

## 2. Inventaire matériel observé (Windows 11 26100)

`Win32_SoundDevice` — statut `OK` au moment de l'audit :

- **MOTU M Series** (MOTU, Inc) — interface audio USB ;
- **Realtek High Definition Audio** ;
- NVIDIA Virtual Audio Device (Wave Extensible), NVIDIA High Definition Audio ;
- Intel Smart Sound (microphones numériques), Nahimic Easy Surround / mirroring, audio Bluetooth.

Des plugins tiers CLAP/VST3 sont installés sur la machine
(`C:\Program Files\Common Files\CLAP`, `…\VST3`), mais **aucun hôte audio n'a été
qualifié ni utilisé pour Ravum**, et Ravum ne produit **aucun** binaire plugin
CLAP/VST3 à ce stade.

## 3. Mesure réelle : gigue d'ordonnancement OS

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

## 4. Ce qui n'a pas été exécuté (et pourquoi)

| Élément du mandat L4-C | Statut | Raison |
|---|---|---|
| Test dans un hôte audio réel (CLAP/VST3/DAW) | **non exécuté** | Ravum ne produit pas encore de binaire plugin CLAP/VST3 ; en créer un est un travail d'ABI distinct (hôte, GUI, paramètres, bus) hors périmètre de ce lot. |
| Périphérique réel via WASAPI/ASIO | **non exécuté** | Nécessite une boucle de rendu COM/WASAPI ou ASIO (FFI `unsafe` significatif) et une sortie audio physique ; non implémenté dans ce cycle, et une exécution réelle aurait des effets audibles non sollicités sur la machine de l'utilisateur. |
| Identification OS / pilote / sample rate / buffer size | **partiel** : OS et périphériques identifiés (§2) ; aucun pilote ni tampon qualifié puisqu'aucun flux audio n'est ouvert. | — |
| Mesure des XRUN | **non exécutée** | Un XRUN est un événement de pilote ; sans pilote ouvert il n'y en a pas à mesurer. Les « dépassements de période » du simulateur ne sont pas des XRUN et sont nommés ainsi. |
| Durée de callback sur périphérique réel | **non mesurée** | idem ; seule la durée de traitement de bloc simulé est mesurée (1–2 µs de calcul pour 256 frames). |
| Swaps répétés dans un callback réel | **non exécuté** | les swaps répétés sont testés dans le simulateur (12 publications, 4 lecteurs, fondu, 0 alloc) mais pas sous contrainte de pilote. |
| Continuation d'état à travers un swap en conditions réelles | **partiel** : démontré dans le simulateur (état possédé par l'hôte, fondu) ; pas sur matériel. | — |
| Reproductibilité des erreurs audio (dropouts, glitches) | **non exécutée** | sans périphérique. |

Ces éléments restent des **travaux non faits**, et aucune conclusion de qualité audio
réelle ne doit en être tirée.

## 5. Plan proposé pour une vraie qualification (non commencé)

1. **Option la moins risquée** : harnais WASAPI *shared mode* en Rust (FFI direct ou crate
   `windows`), boucle de rendu silencieuse (échantillons nuls) pour ne pas produire de son,
   qui appelle `process_rt` dans le callback du pilote. Mesurer durée de callback, lateness,
   XRUN/dropouts, puis swaps répétés sous charge.
2. **Option produit** : construire un plugin CLAP minimal autour de l'ABI de bloc
   (`clap_entry`, process, params) et l'héberger dans un hôte réel (Seno/Reaper/…). Exige
   un ABI hôte distinct et une GUI ; lot séparé.
3. Définir explicitement la plateforme qualifiée (OS, pilote, sample rate, buffer size,
   priorité du thread) et **ne jamais** extrapoler le résultat d'un desktop à une garantie
   hard-RT.
4. Mesurer les XRUN sur une fenêtre longue (minutes), pas sur quelques secondes, et
   publier les distributions, pas seulement le maximum.

## 6. Reproduire la seule mesure existante

```powershell
cd D:\App\Ravum
$env:RAVUM_JITTER_CPU_PERCENT = (Get-CimInstance Win32_Processor).LoadPercentage
cargo run -q -p ravum-dsp-harness --bin ravum-jitter -- --iterations 2000 --period-us 5333 --json
# et le simulateur :
cargo run -q --bin ravum -- dsp fixtures/dsp/iir_v1.rs --next fixtures/dsp/iir_v2.rs --blocks 400 --crossfade-frames 256 --json
```
