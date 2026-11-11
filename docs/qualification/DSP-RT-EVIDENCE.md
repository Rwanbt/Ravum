# DSP / temps réel — dossier de preuves (P2-H)

> **Statut : PREUVES MESURÉES, portée explicitement limitée.**
> Ce document enregistre **ce qui a été exécuté**, quand, sur quelle machine, avec
> quelles commandes, et **ce que chaque chiffre ne prouve pas**.
>
> **Verdict : `PARTIAL`.** Il n'existe dans ce dépôt **aucune** preuve de hard
> real-time, aucun WCET, aucune mesure sur périphérique audio. Conformément à
> EFF-4.6, aucun libellé de ce document ne peut être `RT-CERTIFIED` : le profil
> `rt` est **expérimental** et ne certifie rien.
>
> | Élément | Valeur |
> |---|---|
> | Date des mesures | 2026-11-11 (Europe/Paris) |
> | Branche / HEAD | `feat/lang-core-1.0-rc` @ `5d88ecb` |
> | Version de spécification | `core-1.0-draft.1` (DRAFT, non ratifiée) |
> | Profil évalué | `rt` (expérimental), point d'entrée `main` |
> | Hôte | Windows 11 x86-64, Intel i7-13620H, **16 threads logiques**, ≈15,7 Go RAM |
> | Toolchain | `rustc 1.95.0`, `cargo 1.95.0`, édition de lien MSVC `14.50.35717` |
> | Profil de compilation des tests | `test` (debug, `-C opt-level=0`) |
> | Répertoires de preuve (jetables) | `target/probes/runtime-dsp/`, `target/probes/runtime-verticals/` |

## 0. Conditions de charge — à lire avant tout chiffre

La machine est **partagée avec d'autres agents** qui compilent dans le même
workspace. Le répertoire `target/` est verrouillé par `cargo` : toutes les
mesures ci-dessous ont été lancées **séquentiellement**, jamais en parallèle
d'une autre compilation, avec `--offline`.

| Moment | Charge CPU observée | Effet |
|---|---|---|
| Campagne de stress + suites de tests | non échantillonnée à chaque étape | les bornes de durée des tests sont « généreuses » par conception (60 s/scénario) et **aucune assertion de débit** n'existe |
| Mesure de gigue OS | **0 % au départ, 32 % à la fin** | la gigue mesurée est donc un **majorant** sous charge croissante, pas un plancher |
| Mesure `check`/`build` de l'exemple DSP | **26 % → 46 %** | médianes = **majorants** |
| Mesure du binaire de l'exemple DSP | 1 % | mesure quasi au repos |
| Rejeu de la même campagne 20 min plus tard | plus élevée (autres agents en compilation) | **même code, mêmes tests : 42,60 s** pour les 9 tests (contre 14,92 s) et **28,745 s** pour la campagne (contre 15,235 s) — soit ≈ ×2,9 et ×1,9. C'est la démonstration directe que ces durées mesurent la charge, pas le code : aucun chiffre de durée de ce document n'est une performance |

Aucun chiffre de ce document n'est une mesure sur machine dédiée. Aucun n'est
publié comme un résultat reproductible au sens de
[`REPRODUCIBILITY.md`](REPRODUCIBILITY.md) sans rejeu sur une machine calme.

### 0.1 Provenance des binaires (arbre partagé, en cours de modification)

L'arbre était **édité par d'autres agents pendant les mesures** (`emit.rs`,
`own.rs`, `typeck.rs`, `run.rs`, `test.rs` modifiés entre 11:41 et 11:46). Deux
binaires CLI ont servi :

| Binaire | Construit le | Taille | Mesures concernées |
|---|---|---:|---|
| `target/debug/ravum-lang.exe` @ `5d88ecb` | 2026-11-11 01:22 | 5 214 208 o | premières sondes (profil `rt`, conformance `effects/`) |
| `target/debug/ravum-lang.exe` reconstruit | 2026-11-11 11:41:51 | 5 216 768 o | rejeu des sondes, expérience d'identité de build, latences `check`/`build`, parcours E2E |

Les sondes de capacité et le corpus `effects/` **rejoués sur le second binaire**
rendent des codes identiques (`E-UNSUPPORTED-EFFECT`, `E-RES-UNKNOWN`,
`E-EFF-PROFILE`, acceptation de `rt-clean`), et la conformance `effects/` reste
**16/16 PASS**. Les affirmations comportementales ne dépendent donc pas de l'état
transitoire de l'arbre ; les **chiffres de durée**, eux, sont datés et liés à ces
binaires.

## 1. Ce que le profil `rt` vérifie réellement

`rt` est une **analyse statique de capacités** : elle refuse qu'un effet interdit
soit *atteignable* depuis l'entrée (`crates/ravum-lang-sema/src/effects.rs:143`).
Elle ne mesure ni temps, ni pile, ni allocation réelle.

Sondes exécutées avec `target/debug/ravum-lang.exe`, qui se présente comme
`ravum-lang 0.1.0 (edition core-1.0-draft.1)`, sur les paquets jetables de
`target/probes/runtime-verticals/` :

| Sonde | Source | `--profile` | Résultat mesuré | Exit |
|---|---|---|---|---|
| `rt-clean` | `fn process(x: f64, y: f64) -> f64 effects()` + `main` sans E/S | `rt` | **accepté**, effets `crate::main = []`, `crate::process = []` | 0 |
| `rt-clean` | idem | `pure` | accepté, mêmes effets | 0 |
| `rt-alloc` | `main` appelle une fonction qui construit un `Vec` | `rt` | `E-EFF-PROFILE` — « profile `rt` forbids effect `alloc` reachable from `main` » | 1 |
| `rt-alloc` | idem | `pure` | `E-EFF-PROFILE` — même message, avec `alloc` | 1 |
| `examples/dsp-gain-iir` | gain + IIR + `io::println` dans `main` | `core` | accepté, 0 erreur ; effets `main = [io, alloc]`, `gain = [alloc]`, `iir = [alloc]` | 0 |
| `examples/dsp-gain-iir` | idem | **`rt`** | **1 erreur** : `E-EFF-PROFILE` — « profile `rt` forbids effect `io` reachable from `main` » (note : « offending call: call to `std::io::println` ») | 1 |

Conformance du modèle d'effets (corpus exécutable) :

```
target\debug\ravum-conformance.exe run --filter effects/ --toolchain target\debug\ravum-lang.exe
→ summary: total=16 pass=16 fail=0 skip=0 not_run=0   (durée murale ≈ 1,1 s)
```

dont les cas `effects/profile-rt-ok` (accepté) et `effects/profile-rt-alloc`
(refusé) : le comportement du profil est donc **couvert par le corpus**, pas
seulement par des sondes ad hoc.

**Conséquence honnête** : l'exemple DSP **ne passe pas** `rt` tel qu'il est écrit,
parce que son `main` imprime. Le profil vérifie l'entrée (`main`), pas la fonction
DSP isolée (EFF-4.3, Q-EFF-6). Un « DSP vérifié rt » exigerait soit une entrée
sans E/S, soit de vérifier tout le paquet — ce qui n'est pas le comportement
actuel et n'est pas décidé.

## 2. Campagnes de tests exécutées

Toutes les commandes ci-dessous ont été exécutées depuis la racine du dépôt,
`cargo test --offline`, une suite à la fois.

| # | Commande | Résultat | Durée de la suite | Murale |
|---|---|---|---:|---:|
| 1 | `cargo test -p ravum-dsp-harness --lib` | **6 pass / 0 fail** | 0,00 s | 1,3 s |
| 2 | `cargo test -p ravum-patch-host --test rt_lifecycle` | **5 pass / 0 fail** | 5,27 s | 6,3 s |
| 3 | `cargo test -p ravum-dsp-harness --test dsp_stress -- --test-threads=1` | **9 pass / 0 fail** | 14,92 s | 15,3 s |
| 4 | `cargo test -p ravum-lang-sema --lib` | 142 pass / 0 fail | 0,20 s | 0,7 s |
| 5 | `cargo test -p ravum-lang-sema --test ffi` | 5 pass / 0 fail | 0,04 s | 0,5 s |
| 6 | `cargo test -p ravum-lang-codegen --test backend_audit` | 21 pass / 0 fail | 3,69 s | 4,0 s |
| 7 | `cargo test -p ravum-lang-cli --test cli` | 44 pass / 0 fail | 4,12 s | 4,6 s |
| 8 | `cargo test -p ravum-store` | 6 pass / 0 fail | 0,00 s | non mesurée séparément |

Les lignes 4–7 ne sont pas des tests « temps réel » : elles prouvent que la
chaîne d'effets/codegen/CLI sur laquelle repose le profil `rt` est verte à ce
SHA. La ligne 8 prouve que SQLite fonctionne **côté Rust** (voir
[`RFC-P2-VERTICALS-BLOCKERS.md`](../rfc/RFC-P2-VERTICALS-BLOCKERS.md) §3.4) —
elle n'est pas une preuve de capacité `.rv`.

## 3. Campagne de stress DSP (hôte de patch, modules natifs réels)

Commande d'évidence complète (écrit un JSON) :

```powershell
$env:RAVUM_STRESS_EVIDENCE = "D:\App\Ravum\target\probes\runtime-dsp\dsp-stress-evidence.json"
cargo test --offline -p ravum-dsp-harness --test dsp_stress stress_evidence_campaign -- --nocapture
```

Résultat : **1 test, 0 échec**, campagne complète en **15 235 ms**, 8 scénarios,
RSS du processus de test **6 320 128 → 7 581 696 octets** (Δ = **1 261 568
octets**, soit +1,20 Mio). Budget interne de la campagne : 300 000 ms ; borne par
scénario : 60 s. Aucun scénario n'approche sa borne.

| Scénario | Durée | Chiffres mesurés |
|---|---:|---|
| `ten_thousand_swaps` | 2 777 ms | **10 000 swaps** ; `prepare_us_total = 2 190 097 µs`, `apply_us_total = 54 279 µs` → **≈ 5,43 µs par activation** ; **0** violation d'allocation, **0** violation de libération ; 0 bloc non fini ; crossfade : pire écart échantillon **0,0576** (immédiat : 0,6426 ; référence sinusoïdale : 0,0288) ; `live_modules_after_drain = 1` |
| `concurrent_publications` | 9 732 ms | 2 publieurs, **4 000 activations**, **10 150 536 blocs rendus**, **0 bloc mixte** (chaque bloc provient d'une seule version publiée) |
| `two_hosts_in_parallel` | 996 ms | 500 itérations × 2 hôtes, 0 violation, 0 allocation sur le chemin de rendu de l'hôte A |
| `stop_during_swaps` | 922 ms | 500 swaps concurrents d'un arrêt ; **1 759 358 blocs** rendus, pas d'interblocage (watchdog de progression à 30 s), 0 bloc non fini |
| `saturation` | 557 ms | 3 cycles × 20 modules, pic 20 modules vivants, **RSS delta 0,457 Mio** pour une borne de 256 Mio, 0 module vivant après drain |
| `stop_start_cycles` | 229 ms | 50 cycles, **402 109 blocs** de worker, 0 allocation / 0 libération sur le thread de rendu |
| `drop_during_closure` | 10 ms | 11 087 modules déchargés cumulés, 0 allocation / 0 libération sur le thread de rendu, 0 récupération en attente |
| `variable_buffers` | 8 ms | 16 tailles de bloc (1 → 4 096), 8 008 frames, frames = 0 traité comme no-op, sortie bornée |

Source brute : `target/probes/runtime-dsp/dsp-stress-evidence.json`
(`schema_version: 1`, `kind: dsp-stress-campaign`).

### 3.1 Défaut d'outillage relevé **et corrigé** (2026-11-11)

Le champ `"date"` du JSON d'évidence était une **constante codée en dur**
(`"2026-11-09"`) dans `crates/ravum-dsp-harness/tests/dsp_stress.rs` (bloc
`stress_evidence_campaign`) : tout fichier d'évidence était antidaté, ce qui
viole INV-08 (une preuve porte ses identifiants, dont sa date d'exécution).

**Correction** : `fn utc_date()` calcule la date UTC au moment de l'écriture —
`SystemTime::now()` → jours depuis l'époque Unix → algorithme `civil_from_days`
de H. Hinnant (grégorien proleptique). Aucune dépendance ajoutée.

**Vérifications** :

| Contrôle | Résultat |
|---|---|
| Algorithme comparé à 7 époques connues (1970-01-01, 1970-01-02, 2000-02-29, 2025-01-01, 2026-01-01, 2026-11-11, 2027-01-01) par une sonde `rustc` indépendante (`target/probes/runtime-store/datecheck.rs`) | **0 divergence** |
| Campagne rejouée (`RAVUM_STRESS_EVIDENCE=…`) : champ `"date"` du JSON écrit | **`2026-11-11`**, identique à la date UTC de la machine au moment du run |
| `cargo test --offline -p ravum-dsp-harness --test dsp_stress -- --test-threads=1` | **9/9 pass** (42,60 s sous charge concurrente) |

**Effet secondaire instructif** : la campagne rejouée a duré **28 745 ms** contre
**15 235 ms** au premier run (préparation des 10 000 swaps : 4 952 991 µs contre
2 190 097 µs), sur la même machine et le même code. Les durées de ce document
mesurent la charge de la machine partagée ; elles ne mesurent pas le code. C'est
une raison de plus de n'assertionner que des invariants, jamais un débit.

## 4. Cycle de vie temps réel de l'hôte de patch

`cargo test -p ravum-patch-host --test rt_lifecycle` — **5/5 pass** en 5,27 s :

| Test | Ce qu'il prouve |
|---|---|
| `audio_thread_neither_allocates_nor_frees_during_repeated_swaps` | le thread « audio » ne prend aucune allocation ni libération pendant des swaps répétés |
| `retired_modules_are_freed_off_the_audio_thread_and_nothing_leaks` | la libération d'un module retiré se fait **hors** du thread de rendu |
| `several_audio_threads_and_one_writer_share_one_version_per_block` | plusieurs lecteurs + un écrivain : une seule version visible par bloc |
| `a_long_block_delays_publication_but_the_audio_path_never_waits_on_the_writer` | un bloc long retarde la publication mais le chemin audio n'attend jamais l'écrivain |
| `a_publication_storm_never_leaks_a_native_handle` | pas de fuite de handle natif sous rafale de publications |

## 5. Gigue de l'ordonnanceur OS (mesure de contexte, pas de preuve RT)

```powershell
target\debug\ravum-jitter.exe --iterations 2000 --period-us 5333
```

(`timeBeginPeriod(1)` activé ; grille d'échéances fixe, pas une boucle de sleep
dérivante ; aucune priorité temps réel demandée)

| Statistique | `lateness_us` (retard sur l'échéance) | `interval_us` (période observée) |
|---|---:|---:|
| min | 20 | 1 |
| p50 | **299** | **5 480** |
| p95 | 535 | 5 659 |
| p99 | 637 | 5 776 |
| max | **6 579** | 11 480 |
| échéances manquées (> 1 période) | **1** | — |

Période visée : **5 333 µs** (256 frames @ 48 kHz). Durée murale : 10 666 ms.

**Lecture honnête** : la médiane (299 µs) et le p99 (637 µs) tiennent largement
dans une période de 5 333 µs. Mais le **maximum mesuré (6 579 µs) dépasse la
période** et une échéance a été manquée — sur une machine de bureau, sans
priorité temps réel, avec une charge qui est passée de 0 % à 32 % pendant la
mesure. C'est exactement la démonstration que **la moyenne ne prouve rien** : un
système peut avoir une latence médiane excellente et rater ses échéances.

## 6. Parcours de bout en bout de l'exemple DSP

| Étape | Commande | Résultat |
|---|---|---|
| Vérification | `ravum-lang check examples/dsp-gain-iir` | 0 erreur, 0 avertissement ; effets `[alloc]`, `[alloc]`, `[io, alloc]` |
| Construction release | `ravum-lang build examples/dsp-gain-iir --release` | **1,47 s** ; binaire **153 600 octets** (debug : 175 616) ; source générée **9 464 octets** |
| Exécution | `ravum-lang run examples/dsp-gain-iir` | 8 lignes, valeurs conformes au calcul attendu (`gain ×2` → 2 / 1 / −0,5 / 0 ; IIR `a=0,8 b=0,2` → 0,2 / 0,26 / 0,158 / 0,1264), exit 0 |
| Durée du processus | binaire release, 7 exécutions | médiane **6,82 ms** (6,59 – 7,70 ms) |

**Ce que la dernière ligne signifie** : l'exemple traite **4 échantillons**. Sa
durée d'exécution mesure le démarrage du processus et 8 écritures sur `stdout`,
pas un traitement DSP. La citer comme « performance DSP » serait faux. Aucune
mesure de débit DSP n'est donc rapportée ici.

## 7. Matrice des garanties : couvert / partiel / non couvert

| Garantie visée | Statut | Preuve ou raison |
|---|---|---|
| Aucune allocation sur le chemin de rendu (fenêtre mesurée) | **COUVERT (instrumenté)** | compteur global `CountingAllocator` ; 0 sur 10 000 swaps et sur tous les scénarios ; l'instrument ne voit que le thread mesuré et seulement pendant la fenêtre |
| Aucune libération sur le chemin de rendu | **COUVERT (instrumenté)** | idem, compteur de désallocations |
| Pas de destruction de module sur le thread de rendu | **COUVERT** | `rt_lifecycle`, `drop_during_closure` |
| Activation atomique (un bloc = une version) | **COUVERT** | 10 150 536 blocs, 0 mixte |
| Pas de fuite de modules/handles | **COUVERT** | compteurs `live_modules` / `unloaded_modules` + RSS bornée |
| Crossfade sans discontinuité au-delà de la référence sinusoïdale | **PARTIEL** | pire écart mesuré 0,0576 vs référence 0,0288 : le crossfade **réduit** l'écart immédiat (0,6426) mais ne l'annule pas ; aucune mesure perceptuelle |
| Aucun panic sur le chemin audio | **NON COUVERT** | aucun test n'injecte de panic dans un module chargé ; le harnais ne mesure pas les `catch_unwind` |
| Blocage (locks, I/O) dans le callback | **PARTIEL** | prouvé par construction du code de l'hôte (garde non possédante, pas de verrou tenu pendant le rendu) et par « l'audio n'attend jamais l'écrivain » ; **aucun instrument ne mesure les blocages** (pas de détecteur de lock) |
| Bornes de pile / d'itérations | **NON COUVERT** | aucune analyse ; `rt` ne vérifie pas RT-bounded (`docs/specs/04-temps-reel.md`) |
| WCET | **NON COUVERT** | aucune méthode, aucune plateforme qualifiée |
| Périphérique audio réel, XRUN, WASAPI/ASIO/JACK | **NON COUVERT** | cadence **simulée** (`frames / sample_rate`) ; aucun pilote |
| Priorité temps réel du thread | **NON COUVERT** | jamais demandée (le harnais le dit dans ses `limits`) |
| FFI dans le callback | **NON COUVERT** | aucun module de test n'appelle de l'externe dans le rendu |
| Dépendances du module chargé | **PARTIEL** | les fixtures sont compilées par `rustc` (`ravum-rust-adapter`) ; leur arbre de dépendances n'est pas audité à l'activation |
| Panics intrinsèques (débordement, division par zéro, index) | **NON SUIVI** | approximation assumée EFF-6.4 : ce ne sont pas des effets |
| État préservé au rechargement | **NON COUVERT** | T8 : `watch` = rebuild + restart ; aucun hot swap d'état (`unsupported.md` §5, Q-UNS-5) |

## 8. Ce que ces chiffres ne prouvent pas — explicitement

1. **Une moyenne (ou une médiane, ou un p99) n'est pas une preuve de hard-RT.**
   La gigue médiane mesurée est de 299 µs, ce qui est excellent, et pourtant une
   échéance a été manquée avec un maximum de 6 579 µs (123 % de la période). Seule
   une **borne supérieure prouvée** (WCET + analyse de plateforme) pourrait
   soutenir une revendication hard-RT ; elle n'existe pas.
2. **Le harnais simule la cadence.** `frames / sample_rate` n'est pas une
   horloge audio : il n'y a ni pilote, ni tampon, ni XRUN, ni gigue de device.
   Les chiffres décrivent le **code du chemin de rendu**, pas un système audio.
3. **Le compteur d'allocation mesure une fenêtre et un thread.** Un zéro prouve
   qu'aucune allocation n'a eu lieu dans cette fenêtre sur ce thread ; il ne
   prouve rien sur les allocations d'un autre thread, ni sur un chemin non exercé.
4. **Les builds sont en mode debug** (`opt-level=0`) pour les tests ; les
   durées de scénarios sont donc pessimistes et non comparables à un build
   release. Aucune conclusion de performance n'est tirée de ces durées.
5. **La plateforme est une seule machine de bureau**, sous charge d'autres
   agents. Aucune mesure Linux, ARM, ni sur cible embarquée.
6. **Le profil `rt` est un refus statique.** Il n'instrumente rien, n'ajoute
   aucun jeton de capacité à l'exécution (EFF-5.1) et ne certifie pas l'absence de
   panic (EFF-4.5, EFF-6.4).
7. **`alloc` est une sur-approximation statique** (EFF-2.5) : un programme peut
   être marqué `alloc` sans allouer, et — plus important — le profil `rt` ne
   *mesure* pas l'absence d'allocation ; il l'exige par construction en
   interdisant `alloc` et `String`/`Vec`.

## 9. Non exécuté (`NOT_RUN`) et limites

| Élément | Statut | Raison |
|---|---|---|
| Mesure sur périphérique audio (WASAPI/ASIO/JACK/PipeWire/CoreAudio) | `NOT_RUN` | aucun pilote dans le dépôt ; hors périmètre Core 1.0 |
| XRUN réels | `NOT_RUN` | idem |
| Priorité temps réel (`THREAD_PRIORITY_TIME_CRITICAL`, `SCHED_FIFO`) | `NOT_RUN` | jamais demandée ; une machine de bureau non privilégiée ne la garantit pas |
| WCET d'une fonction DSP | `NOT_RUN` | aucune méthode ni plateforme qualifiée |
| Analyse de bornes de pile/itérations | `NOT_RUN` | non implémentée |
| Tests Linux des scénarios DSP | `NOT_RUN` | qualification Linux existante pour la CI, mais ces campagnes n'ont pas été rejouées ici sur Linux |
| Message de gigue sur machine **calme** | `NOT_RUN` | la mesure a été prise sous charge croissante (0 % → 32 %) ; un rejeu au repos est nécessaire pour un plancher |
| Panic injecté dans un module chargé | `NOT_RUN` | pas de scénario correspondant |
| Instrumentation des blocages (locks) | `NOT_RUN` | aucun détecteur dans le dépôt |
| Rejeu de la campagne de 10 000 swaps sur un build release | `NOT_RUN` | non nécessaire aux invariants (qui ne dépendent pas de la vitesse) |

## 10. Verdict

| Objet | Verdict | Libellé autorisé |
|---|---|---|
| Profil `rt` (existence, refus, couverture conformance) | **`PARTIAL`** | « profil `rt` **expérimental**, vérifié statiquement, non certifié » |
| Chemin de rendu (allocation/libération/destruction/activation) | **`PARTIAL`** | « invariants mesurés en mémoire sur des modules natifs réels, sans périphérique audio » |
| Gigue OS | **`PARTIAL`** | « caractérisation de l'ordonnanceur sous charge, pas une qualification » |
| Exemple de bout en bout `dsp-gain-iir` | **`TESTED`** (et non `PROVED`) | « exemple exécuté ; ne passe pas `--profile rt` ; hors callback » |
| Hard-RT / WCET | **`NOT_APPLICABLE`** | aucune revendication possible |

Conformément à EFF-6.7, tout rapport dérivé de ce document **MUST** déclarer : la
version de spécification (`core-1.0-draft.1`), le profil (`rt`, expérimental), le
point d'entrée (`main`), le résultat (`PARTIAL`) — et **jamais** `PROVED` ni
`RT-CERTIFIED`.
