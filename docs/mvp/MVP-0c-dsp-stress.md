# MVP-0c / L4.3 — campagne de stress DSP

Statut : **exécutée le 2026-11-09** sur la machine de référence (Windows 11 26100,
16 CPU logiques, rustc 1.95.0, profil test/debug), en mémoire, sans périphérique audio.
Preuve machine : `evidence/mvp0c-dsp-stress-2026-11-09.json` (8 cas mesurés, 33 999 ms de
campagne, RSS 6,4 → 7,4 Mio).

Ce document ne revendique **aucune** qualification hard-réaliste. Il prouve des
invariants de cycle de vie et d'allocation sur un vrai `BlockHost` avec de vrais modules
natifs compilés par `rustc`, pas un modèle.

## 1. Objectif

Le mandat MVP-0c L4.3 demande de couvrir les cas limites laissés ouverts par MVP-0a/0b :
swaps massifs, instances multiples, buffers variables, réinitialisation, destruction
pendant la fermeture, arrêt du worker pendant un swap, saturation des modules et races
entre état DSP et versions publiées. Les 15 tests existants (6 unités du harnais, 9
unités XRUN de la sonde audio) sont conservés et verts.

## 2. Méthode

- **Vrais modules** : `fixtures/dsp/{gain_v1,gain_v2,iir_v1,iir_v2}.rs` sont compilés par
  `RustAdapter` (rustc réel), puis chargés/validés par `ravum-patch-host` exactement comme
  l'application. Aucun mock de module, aucun stub de publication.
- **Vrai hôte** : `BlockHost` (slot de publication sans verrou, publications atomiques,
  guards non possédants), `apply`/`prepare_block` réels, réclamation différée réelle
  (`ravum-reclaimer`).
- **Compteurs d'allocation** : l'allocateur global de `ravum-dsp-harness` compte les
  allocations/désallocations **par thread**. Chaque bloc du thread de rendu est mesuré :
  `reset_thread_counters()` puis `process_rt` puis lecture. Le compteur est local au
  thread, donc sûr même si les tests tournent en parallèle.
- **Compteurs de modules** (vivants / déchargés / en attente) : globaux au processus, donc
  les scénarios qui les vérifient prennent un verrou global et s'exécutent un par un.
  Chaque scénario a une borne dure de 60 s ; la campagne complète est bornée à 5 min.
- **Watchdogs** : les scénarios qui pourraient s'interbloquer (arrêt pendant un swap,
  races) arment un watchdog **de progression** : il n'abandonne le processus que si le
  compteur de progrès (blocs rendus + activations réussies, mis à jour à chaque étape) ne
  bouge plus pendant 30 s. Une machine lente mais qui progresse n'est jamais tuée ; un
  vrai interblocage devient un échec visible. Aucun watchdog ne s'est déclenché depuis le
  correctif CI (§8).
- **Dépendances de test** : avum-rust-adapter et avum-snapshot sont ajoutés en\n  [dev-dependencies] du crate (comme pour les tests M3 de avum-patch-host), pour\n  compiler les fixtures réelles ; aucune dépendance de production n'est ajoutée.\n- **Images chaudes** : la boucle de 10 000 swaps garde une référence sur chaque module
  candidat, donc l'image native reste mappée et chaque itération mesure le **chemin de
  swap/publication**, pas 10 000 chargements à froid du chargeur Windows. Le chargement à
  froid (20 modules distincts × 3 cycles) est mesuré séparément dans le cas saturation.
  Cette séparation est explicite : le premier chiffre ne dit rien du coût de `LoadLibrary`.

## 3. Cas → test → résultat

Tous les tests sont dans `crates/ravum-dsp-harness/tests/dsp_stress.rs` ; commande :
`cargo test -p ravum-dsp-harness --test dsp_stress`.

| Cas | Test | Résultat | Chiffres mesurés |
|---|---|---|---|
| 1. 10 000 swaps pendant un rendu continu | `ten_thousand_swaps_keep_the_callback_finite_alloc_free_and_one_version` | **passe** | 10 000 swaps + 1 publication sous guard = 10 001 activations, version finale 10 002 ; 0 bloc non fini ; 0 allocation / 0 désallocation sur le thread de rendu ; sortie bornée (max 1,0) ; continuité crossfade 256 : `\|delta\|` max 0,0576 avec fondu contre 0,6426 sans (référence sinusoïdale 0,0288) ; module retiré resté mappé sous guard ; 20 455 ms |
| 2. Instances multiples en parallèle | `two_hosts_in_parallel_keep_independent_state` | **passe** | hôte A (gain 0,5/2,0) : 500 activations, 0 valeur étrangère, 0 allocation ; hôte B (IIR 0,1/0,5) : 500 activations, 0 violation, sortie max 0,4995 ; aucun état partagé ; 1 153 ms |
| 3. Stop/start répétés, hôte réutilisé | `stop_start_cycles_reuse_the_host_without_leaking_modules` | **passe** | 50 cycles, 50 activations, 401 786 blocs rendus par les workers, 50 blocs de contrôle après arrêt ; 0 allocation/0 désallocation sur le thread de rendu ; modules vivants 3 → 1 après drainage (aucune fuite), 0 en attente ; 247 ms |
| 4. Buffers variables (+ changement de version) | `variable_block_sizes_are_defined_across_a_version_change` | **passe** | tailles 1, 7, 63, 64, 65, 127, 128, 129, 255, 256, 257, 511, 512, 513, 1024, 4096 (8 008 frames) ; 2 blocs de 0 frame = no-op défini (sortie intacte) ; version changée au milieu : chaque bloc entier vient d'une seule version ; crossfade avec blocs 1000/2000/4096 : 0 allocation, sortie dans l'enveloppe [0,5 ; 2,0] ; 9 ms |
| 5. Destruction pendant/après fermeture | `dropping_the_host_after_stop_never_destroys_on_the_render_thread` | **passe** | 955 blocs rendus, 1 swap avec crossfade, worker arrêté puis hôte `drop` sur le thread de contrôle ; 0 allocation/0 désallocation sur le thread de rendu ; reclaimer : 11 083 → 11 085 modules déchargés, 0 module vivant, 0 en attente après drainage ; 9 ms |
| 6. Arrêt du worker pendant un swap | `stop_during_swaps_terminates_without_deadlock` | **passe** | 500 swaps pendant 1 888 092 blocs rendus, arrêt après la dernière publication ; aucun bloc non fini ; pas d'interblocage (watchdog de progression 30 s non déclenché) ; 1 107 ms |
| 7. Saturation des modules | `saturation_dozens_of_modules_are_reclaimed_and_memory_stays_bounded` | **passe** | 20 modules distincts × 3 cycles = 60 chargements à froid ; pic 20 modules vivants, 0 après chaque cycle, 0 en attente ; RSS +0,54 Mio (borne 256 Mio) ; 596 ms |
| 8. Races état DSP / versions publiées | `concurrent_publications_render_whole_versions_only` | **passe** | 2 threads publieurs, 4 000 activations, 22 082 020 blocs rendus pendant les races, **0 bloc mixé** (chaque bloc entièrement gain 0,5 ou 2,0) ; 10 420 ms |
| — | `stress_evidence_campaign` | **passe** | campagne compacte toujours exécutée (8 scénarios, ~2,8 s) ; écrit la preuve JSON complète quand `RAVUM_STRESS_EVIDENCE` est fourni |

Campagne d'évidence complète : 8 cas, **22 131 ms** au total, chaque cas < 60 s
(le plus long : races de publication, 13 246 ms), suite de tests complète
`cargo test -p ravum-dsp-harness` : **24 tests verts** (6 unités harnais + 9 unités XRUN
de la sonde + 9 tests de stress), 36 s pour le binaire de stress.

## 4. Ce que chaque cas prouve — et ne prouve pas

**Cas 1.** Prouve que 10 000 publications réelles sur des modules natifs ne produisent
aucun échantillon non fini, aucune allocation sur le thread de rendu, et que la
publication sous guard ne libère pas le module en cours de lecture (`ArcSlot::store`
attend la fin des lecteurs, comportement vérifié par le test : le publieur tourne sur un
autre thread pendant que le guard lit le module retiré, puis aboutit). La mesure de
continuité crossfade est la même métrique que L4-B (delta inter-échantillon borné).
*Ne prouve pas* : le coût du chargement natif à froid d'une grosse fixture (mesuré au
cas 7) ; une borne de latence sur un vrai périphérique. Les modules du cas 1 sont
volontairement minimaux (~9 Kio) : le hachage d'artefact de prepare_block dominait le
coût (31,2 s / 31,9 s mesures avec les fixtures de 102 Kio), et le test doit rester dans
le budget CI. Le chemin de validation est le même, seule la taille binaire change.

**Cas 2.** Prouve que deux `BlockHost` indépendants ne partagent aucun état : chaque hôte
ne rend que ses versions, aucune valeur croisée, aucune allocation. *Ne prouve pas* une
isolation mémoire au sens OS (même processus, même allocateur compteur).

**Cas 3.** Prouve que l'hôte est réutilisable après arrêt du worker, 50 fois, sans fuite
de modules et sans allocation sur le thread de rendu. *Ne prouve pas* la réutilisation
après un crash du worker (pas simulé : un panic dans le module n'est pas couvert, l'ABI
interdit le panic à travers la frontière).

**Cas 4.** Définit le comportement des tailles variables, y compris `frames == 0`
(no-op explicite) et les blocs plus grands que `MAX_FADE_FRAMES` (fondu découpé, mélange
borné). *Ne prouve pas* une politique produit pour les tailles non multiples de la taille
de bloc d'un hôte réel (l'ABI de bloc accepte toute taille ≥ 1).

**Cas 5.** Prouve que la destruction de l'hôte (et donc des modules actifs) se fait hors
du thread de rendu : les compteurs du thread de rendu restent 0/0, les modules sont
déchargés par le reclaimer, et rien ne reste vivant/en attente. La preuve directe du
*thread* qui exécute `Drop` n'est pas instrumentée ; elle repose sur l'API non possédante
(`enter_rt`) + le reclaimer dédié, tous deux exercés ici.

**Cas 6.** Prouve l'absence d'interblocage dans cette séquence (swaps concurrents + arrêt),
avec une borne dure si le défaut apparaissait. *Ne prouve pas* l'absence de deadlock dans
toutes les interleavings possibles (pas de model-checking).

**Cas 7.** Prouve que des dizaines de modules vivants simultanément se réclament en 3
cycles, sans fuite de handle, avec une RSS stable (+0,54 Mio pour 20 modules). *Ne prouve
pas* une borne mémoire pour des milliers de modules ni pour des modules plus gros que les
fixtures (les fixtures pèsent quelques dizaines de Kio).

**Cas 8.** Prouve qu'un bloc ne mélange jamais deux versions publiées : chaque bloc est
lu sous un guard unique et uniforme (gain 0,5 ou 2,0), sur 22 millions de blocs pendant
4 000 publications concurrentes. Le crossfade n'est **pas** armé ici (mélange volontaire
borné, couvert au cas 1). *Ne prouve pas* l'absence de tornade de publication côté
ordonnanceur : les writers sont sérialisés par le mutex du host.

## 5. Limites et non-claims

- **En mémoire uniquement** : pas de périphérique audio, pas de priorité temps réel, pas
  de XRUN, pas de mesure de WCET ni de page faults. La sonde `ravum-audio-probe` reste la
  mesure « device » du MVP-0b.
- **Compteurs d'allocation** : zéro prouve qu'aucune allocation n'a eu lieu sur le thread
  mesuré pendant la fenêtre ; cela ne prouve pas l'absence de blocage système, de faute de
  page ou de préemption.
- **Compteurs de modules globaux au processus** : les scénarios sont sérialisés ; les
  chiffres ne sont pas comparables entre processus concurrents.
- **Machine partagée** : deux autres agents compilaient en parallèle ; aucun test
  n'affirme un débit ou une durée nominale, seulement des invariants et des bornes larges.
- **Windows uniquement** : les fixtures natives et le compteur RSS utilisent le toolchain
  MSVC/Windows ; les invariants Rust des scénarios sont, eux, portables.
- **Watchdog** : un interblocage réel est transformé en abandon du processus de test
  (borne dure, 30 s sans progrès) ; ce n'est pas un mécanisme de reprise gracieuse. Une
  machine simplement lente n'est pas concernée : le compteur de progrès continue
  d'avancer.

## 6. Reproduire

```powershell
cd D:\App\Ravum
$env:CARGO_BUILD_JOBS = '2'

# Suite complète (24 tests : 6 harnais + 9 XRUN sonde + 9 stress) :
cargo test -p ravum-dsp-harness

# Seulement la campagne de stress (9 tests, ~36 s) :
cargo test -p ravum-dsp-harness --test dsp_stress -- --nocapture --test-threads=1

# Preuve JSON complète (relance la campagne, écrit le fichier) :
$env:RAVUM_STRESS_EVIDENCE = 'D:\App\Ravum\evidence\mvp0c-dsp-stress-2026-11-09.json'
cargo test -p ravum-dsp-harness --test dsp_stress stress_evidence_campaign -- --nocapture --test-threads=1
```

La campagne requiert `rustc` dans le `PATH` (elle compile les fixtures, comme les tests
M3 existants de `ravum-patch-host`). Aucun son n'est produit : aucun périphérique n'est
ouvert. Aucun processus n'est laissé après les tests (workers joints, watchdogs désarmés).

## 7. Preuves

- `evidence/mvp0c-dsp-stress-2026-11-09.json` : campagne complète (8 cas), durées,
  compteurs alloc/free, modules vivants/déchargés, RSS, limites et non-claims.
- Les preuves JSON des mesures device restent celles de MVP-0b
  (`evidence/mvp0b-audio-*.json`), inchangées.

## 8. Défaut CI et correctif (2026-11-09)

### 8.1 Symptôme exact

Run GitHub Actions 42543351768, commit `c86d1f0` : `cargo test --workspace` lance
`tests/dsp_stress.rs`, affiche `running 9 tests`, puis après ~46 s le processus meurt
avec `exit code: 0xc0000409, STATUS_STACK_BUFFER_OVERRUN` ; la dernière ligne visible est
`concurrent_publications_render_whole_versions_only ...`. Localement (16 Go, 10 cœurs)
les 9 tests passaient (36,25 s). CI = Windows Server 2025, 4 vCPU, 7 Go.

### 8.2 Cause racine (test, pas harnais)

Ce n'est **ni de l'UB, ni un débordement de pile, ni un défaut du harnais** : c'est le
watchdog du test qui appelle `std::process::abort()`. Sur Windows MSVC, `abort()` passe
par `__fastfail` et produit exactement `0xC0000409`.

Chaîne causale, reproduite par le Lead sous charge :

1. les deux threads publieurs exécutaient **la même séquence de candidats en lockstep**
   (`v2, v1, v2, v1, ...`) ; l'un gagnait la publication, l'autre recevait
   `AlreadyActive` ; les deux ré-essayaient, retombaient sur le même candidat, et ainsi
   de suite : le perdant épuisait ses 64 tentatives sans jamais publier ;
2. il paniquait alors (`assert!(activated, ...)`, ligne 1100 du fichier d'origine) ;
3. le `join()` du publieur paniquait à son tour (ligne 1140), le scope se déroulait,
   mais le thread de rendu bouclait encore sur `publishers_done == false` ;
4. le watchdog à 45 s (mur-à-mur) constatait que le scope ne rendait pas la main et
   appelait `abort()` → `0xC0000409` après ~45 s d'attente (d'où le « ~46 s » du run).

Différence CI/local : avec 10 cœurs et un ordonnanceur plus jittery, le lockstep se
cassait au bout de quelques essais (le perdant finissait par gagner une course), donc le
test passait. Sur le runner 4 vCPU plus lent, les deux threads restent suffisamment
synchrones pour que le perdant n'y arrive jamais : le bug de test, latent, devenait
systématique. Le harness (`ArcSlot`, `BlockHost`, reclaimer) n'est pas en cause : les
tests M3 de `ravum-patch-host` et les 8 autres scénarios n'ont jamais échoué.

### 8.3 Correctif

- **Phases opposées déterministes** : publieur 0 commence par `gain_v2`, publieur 1 par
  `gain_v1`, puis chacun alterne. Après un refus (`AlreadyActive`), la tentative suivante
  vise l'autre artefact, qui ne peut pas être actif à cet instant : elle réussit
  toujours. Le nombre de tentatives est borné (4 × activations, minimum 64) mais aucun
  scénario ne doit l'atteindre ; il n'y a plus de panique dans un thread de test.
- **Pas de panique dans un thread secondaire** : `publisher_loop` retourne
  `Result<u64, String>` ; un échec est remonté au thread principal et provoque **une
  seule** panique, après que le rendu a été arrêté.
- **Arrêt garanti du rendu** : un garde `StopOnDrop` met `publishers_done = true` même si
  un publieur panique, donc le `join()` du scope ne peut plus se bloquer indéfiniment.
- **Watchdog de progression, plus de timeout mur-à-mur** : un compteur
  (blocs rendus + activations réussies) est mis à jour à chaque étape ; le watchdog
  n'appelle `abort()` que si ce compteur ne bouge plus pendant **30 s** (vrai
  interblocage). Une machine lente mais qui progresse n'est jamais tuée.
- **Rendu coopératif** : le thread de rendu fait `yield_now()` à chaque bloc pour ne pas
  monopoliser un cœur sur 4 vCPU.

### 8.4 Compromis assumé pour tenir le budget CI

- **Invariant conservé** : 2 publieurs concurrents, **4 000 activations réussies**,
  chaque bloc entièrement d'une seule version (aucun mélange observé), et la boucle de
  **10 000 swaps** du cas 1.
- **Ce qui change** : les activations du cas 8 sont comptées comme publications
  **réussies** (une collision `AlreadyActive` coûte une tentative et est immédiatement
  suivie d'une tentative gagnante) ; il n'y a plus de panique sur épuisement de retries.
  Le cas 1 utilise des modules `no_std` générés (~9 Kio) au lieu des fixtures de 102 Kio,
  pour que le hachage d'artefact obligatoire de `prepare_block` reste dans le budget
  (31,2 s → 3,6 s sur la machine de référence) ; ABI, manifeste, identité d'artefact,
  publication et réclamation sont identiques.
- **Preuve de non-affaiblissement** : les 8 scénarios passent toujours, le cas 8 vérifie
  toujours 4 000 activations concurrentes et l'absence de bloc mixé, le cas 1 vérifie
  toujours 10 000 swaps, 0 allocation sur le thread de rendu et la continuité crossfade.

### 8.5 Marge mesurée (machine de référence, 16 Go / 10 cœurs)

| Mesure | Avant correctif | Après correctif |
|---|---|---|
| local, 4 vCPU simulés (affinité 0xF), cas 8 seul | — | **12,3 s** (watchdog non déclenché) |
| local, 4 vCPU simulés, cas 1 seul | 22,8 s (fixtures 102 Kio) | **9,5 s** (modules ~9 Kio) |
| local, sous charge (2 busy-loops PowerShell), suite stress complète | 62,1 s sous charge, après le seul correctif watchdog et avant les modules minimaux (9/9 verts, mais marge faible) ; watchdog déclenché avant correctif selon le Lead | **38,1 s, 9/9 verts** (cas 8 : 19,7 s, cas 1 : 4,4 s) |
| local, immédiatement après, sans charge | 68,9 s (même état intermédiaire) | **32,2 s, 9/9 verts** (cas 8 : 16,0 s, cas 1 : 5,9 s) |
| campagne d'évidence (8 cas) | 33,999 s | **22,131 s** (cas 1 : 4,5 s, cas 8 : 13,2 s) |

Aucun watchdog ne s'est déclenché après le correctif, y compris sous charge. La preuve
détaillée est `evidence/mvp0c-dsp-stress-ci-2026-11-09.json`.

### 8.6 Ce qui reste vrai / non revendiqué

- Le harnais n'a pas été modifié : le défaut était dans le test (hypothèse de course
  lockstep + watchdog mur-à-mur), pas dans le code produit.
- Un vrai interblocage est toujours transformé en `abort()` après 30 s sans progrès :
  c'est un dernier recours assumé, pas une reprise gracieuse.
- Le test CI ne prouve toujours rien du temps réel audio : il reste un scénario mémoire
  (aucun périphérique, aucune priorité RT, aucun XRUN).
