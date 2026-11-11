# MVP-0b / L1-A — Cycle de vie temps réel des modules DSP

> **Portée** : durcissement du chemin audio du démonstrateur de blocs (M3). Ce document
> décrit un risque réel, la démonstration déterministe de son mécanisme, la correction
> retenue, les preuves mesurées et les limites qui restent. **Aucune qualification
> hard-RT n'est revendiquée** : pas de périphérique audio, pas de priorité de thread,
> pas de mesure de WCET ni de jitter d'ordonnancement.
>
> Machine : Windows 11 Pro 26100, i7-13620H (10 cœurs / 16 threads), 15,71 Go RAM,
> `rustc 1.95.0`, `opt-level=1` pour les fixtures DSP. Campagnes faites pendant que
> d'autres agents tournaient : la charge au moment des mesures est indiquée.

## 1. Le risque, tel qu'il existait dans le code

Le chemin audio de MVP-0a :

```rust
pub fn process_rt(&self, input: &[f32], output: &mut [f32], state: &mut DspState) -> usize {
    let module = self.slot.load();   // ← Arc<T> possédé par le thread audio
    module.process_block(input, output, state)
}                                     // ← libération de l'Arc ici, sur le thread audio
```

`ArcSlot::load` incrémente le compteur fort de l'`Arc` avant de le rendre. Un écrivain
concurrent qui publie une nouvelle version retire l'ancienne. Si, entre l'instant où le
callback a chargé l'ancienne version et l'instant où il la relâche, l'écrivain relâche
**ses propres** références (le `retired` rendu par `store`, et l'`active` chargé pendant
`apply`), alors la référence du callback est la **dernière** : le `Drop` de
`BlockModule` s'exécute sur le thread audio. Or ce `Drop` :

1. libère l'allocation du `BlockModule` (appel à l'allocateur global) ;
2. détruit le champ `_lib: DeferredLibrary` ;
3. `DeferredLibrary::drop` incrémente un compteur atomique, puis **envoie la
   `Library` dans un `mpsc::channel`** (allocation de nœud + verrou interne du canal),
   et c'est le thread de reclamation qui exécute `FreeLibrary`/`dlclose`.

Conclusion exacte : le thread de reclamation garantit que le **déchargement de la
bibliothèque** n'a pas lieu sur le callback ; il ne garantit **pas** que la chaîne de
destruction déclenchée par le dernier `Arc` soit temps réel. La documentation M3 disait
« libération déléguée à un thread dédié » sans qualifier cette chaîne ; c'est corrigé ici.

## 2. Démonstration déterministe du mécanisme

Un test unitaire ne dépendant d'aucune course (`slot.rs`,
`owning_load_can_drop_on_the_reader_thread`) force l'entrelacement :

1. un thread « lecteur » fait un `load_owning()` et attend ;
2. le thread de contrôle publie une nouvelle valeur et relâche toutes ses références ;
3. le lecteur relâche la sienne : c'est la dernière.

Le test enregistre l'identité du thread qui exécute le `Drop` du payload et vérifie qu'il
s'agit du **thread lecteur**. Il passe sur l'ancienne API : le risque est donc réel, pas
hypothétique.

Le test end-to-end `audio_thread_neither_allocates_nor_frees_during_repeated_swaps`
(199 517 blocs, 12 publications) **ne discrimine pas** les deux implémentations : exécuté
avec l'ancien `load()`, il passe aussi. La fenêtre de course est en effet de l'ordre de
la sous-microseconde (deux `Arc::drop` côté écrivain) contre un bloc de quelques
microsecondes ; un test de propriété ne peut pas prétendre la toucher à coup sûr. C'est
pourquoi la preuve du mécanisme est le test déterministe ci-dessus, et la preuve de la
correction est **structurelle** : la nouvelle API ne permet plus au callback de posséder
une référence.

## 3. Correction retenue : garde de lecture non possédante

Deux API de lecture, désormais explicitement nommées :

| API | Coût | Peut être le dernier détenteur ? | Autorisation |
|---|---|---|---|
| `ArcSlot::load_owning() -> Arc<T>` | `fetch_add`/`fetch_sub` + compteur fort | **oui** | threads de contrôle uniquement |
| `ArcSlot::enter() -> ReadGuard<'_, T>` | deux atomiques, aucun compteur fort | **non, par construction** | chemin audio |

`BlockHost::process_rt` utilise `enter()` ; `BlockHost::enter_rt()` expose la même garde
aux appelants qui veulent mesurer un bloc. `BlockHost::enter()` (possédant) reste
disponible mais documenté « thread de contrôle uniquement ».

Propriété clé : un `ReadGuard` est `!Send` (il contient un `*const T`), donc il ne peut
pas être transféré à un autre thread ; il se relâche sur le thread qui l'a pris, et ce
relâchement ne fait qu'un `fetch_sub` sur le compteur de lecteurs.

### Pourquoi le compteur de lecteurs suffit

L'écrivain fait `swap` puis attend `readers == 0` avant de rendre l'`Arc` retiré. Un
lecteur annonce sa présence **avant** de charger le pointeur ; donc si un lecteur a pu
observer l'ancien pointeur, son incrément précède le `swap` de l'écrivain, et le contrôle
`readers == 0` de l'écrivain — postérieur au `swap` — observe nécessairement cet incrément
(ou bien le lecteur a déjà terminé). Dans les deux cas, l'écrivain ne libère pas le
payload sous un lecteur : il le libère lui-même, sur son propre thread.

### Ordres mémoire

Tout est `SeqCst`. `Acquire`/`Release` suffiraient (chargement du pointeur en `Acquire`
côté lecteur, `swap` en `AcqRel`, attente en `Acquire` côté écrivain), mais la cohérence
séquentielle coûte quelques nanosecondes par appel — négligeable devant le bloc mesuré à
1–2 µs — et rend le raisonnement « `readers == 0` après le `swap` » vérifiable sans
modèle formel. C'est un compromis explicite, pas un oubli.

### Alternatives étudiées et rejetées pour ce jalon

| Alternative | Raison du rejet |
|---|---|
| Reclamation par époque / RCU complet avec périodes de grâce | Plus puissant, mais introduit un ordonnanceur de périodes et une API de section critique à qualifier ; surcoût de conception non justifié par la mesure actuelle (un seul écrivain, blocs bornés). |
| Hazard pointers | Même remarque ; implémentation correcte non triviale, et le compteur annoncé suffit pour un seul écrivain sérialisé. |
| Retire queue préallouée | Utile si l'écrivain ne devait jamais attendre ; ici l'attente de l'écrivain est bornée par un bloc et n'est **pas** sur le chemin audio. La file serait un second mécanisme à qualifier. |
| File de reclamation sans attente + libération différée | Conservée comme piste si l'attente de l'écrivain devient un problème mesuré (publication à très haute fréquence). Non implémentée : pas de besoin démontré. |
| Garder `load()` et « supposer » que le callback n'est jamais le dernier | Refusé : c'est exactement l'hypothèse non testable que le mandat interdit. |

## 4. Le mécanisme de reclamation reste, mais il n'est plus atteignable depuis l'audio

`DeferredLibrary::drop` fait toujours un `send` (allocation + verrou) : c'est acceptable
car, par construction, un payload contenant une `DeferredLibrary` ne peut plus être
détruit que par un thread de contrôle. Deux durcissements ajoutés :

1. `init_reclaimer()` est appelé à la construction de l'hôte **et** à chaque
   `DeferredLibrary::open` : le thread de reclamation existe avant le premier `Drop`. Sans
   cela, le premier `Drop` de bibliothèque ailleurs dans le processus aurait créé le
   thread (opération non bornée) sur le thread appelant.
2. Le canal est un `OnceLock<Sender>` statique dont le récepteur appartient au thread de
   reclamation pour la vie du processus : il ne se ferme jamais, donc la branche de repli
   « déchargement inline » est inatteignable en pratique. Elle est conservée pour
   comptabiliser proprement un hypothétique échec d'envoi.

## 5. Invariants du chemin audio et leur statut

| Invariant (mandat L1-A) | Application | Statut mesuré |
|---|---|---|
| Aucune allocation sur le callback | `ReadGuard` = 2 atomiques ; aucun `Arc` fort créé | mesuré, 0 sur 199 517 blocs |
| Aucune désallocation sur le callback | `ReadGuard` ne touche pas au compteur fort | mesuré, 0 sur 199 517 blocs |
| Aucun `Drop` imprévisible | le payload ne peut plus être détruit par un lecteur | démontré par test unitaire + conception |
| Aucun mutex bloquant | aucun verrou dans `process_rt`/`enter_rt` | par construction (revue de code) |
| Aucun accès disque | aucun I/O dans le chemin | par construction |
| Aucun chargement/déchargement de bibliothèque | chargement hors callback dans `prepare_block`, déchargement sur le reclaimer | mesuré, 0 désallocation callback |
| Aucune compilation, aucun JIT | hors callback uniquement | par construction |
| Aucune attente du thread d'intégration | le lecteur n'attend jamais l'écrivain | test : `a_long_block_delays_publication_but_the_audio_path_never_waits_on_the_writer` |

## 6. Tests exécutés

`cargo test -p ravum-patch-host` → **21 tests réussis**, 0 échec (8 unitaires, 8 `block_host`,
5 `rt_lifecycle`).

| Test | Ce qu'il vérifie |
|---|---|
| `slot::owning_load_can_drop_on_the_reader_thread` | démontre le risque de l'API possédante (Drop sur le thread lecteur) |
| `slot::a_guard_never_performs_the_last_drop_and_the_writer_waits_for_it` | la garde non possédante : Drop sur l'écrivain, attente observée ≥ 30 ms |
| `slot::the_last_leaving_guard_decides_when_the_writer_may_free` | plusieurs lecteurs : la libération attend le dernier |
| `reclaimer::a_burst_of_retired_libraries_is_drained_and_accounted_for` | saturation : 200 bibliothèques retirées, file drainée, 0 fuite |
| `reclaimer::init_reclaimer_is_idempotent` | création explicite, idempotente |
| `audio_thread_neither_allocates_nor_frees_during_repeated_swaps` | 12 publications pendant 199 517 blocs : **0 allocation, 0 désallocation** sur l'audio, les deux versions observées |
| `retired_modules_are_freed_off_the_audio_thread_and_nothing_leaks` | 20 publications, 0 désallocation audio, `pending == 0`, `live_modules` revient à la baseline après fermeture |
| `several_audio_threads_and_one_writer_share_one_version_per_block` | 4 lecteurs + 1 écrivain, un seul module par bloc, 0 allocation/désallocation par lecteur |
| `a_long_block_delays_publication_but_the_audio_path_never_waits_on_the_writer` | l'écrivain attend ≥ 40 ms, le chemin audio < 30 ms |
| `a_publication_storm_never_leaks_a_native_handle` | 40 publications en 141,5 ms sans lecteur, backlog drainé, 0 fuite |

Log brut : `evidence/mvp0b-l1a-rt-lifecycle.txt`.

## 7. Mesures

### Campagne end-to-end (test `audio_thread_neither_allocates_nor_frees_during_repeated_swaps`)

```text
[l1a] e2e campaign: blocks=199517 swaps=12 allocations_on_audio_thread=0
      deallocations_on_audio_thread=0 versions_seen={"dsp-gain-v1","dsp-gain-v2"}
[l1a] publication storm: 40 publications in 141.5226ms without any reader
```

### Sessions DSP instrumentées (`ravum dsp`, 400 blocs × 256 frames, 48 kHz)

Champ `deallocations_on_audio_thread` ajouté au rapport (additif, `schema_version` inchangé,
`#[serde(default)]` : un ancien rapport sans le champ désérialise et signifie « non mesuré »,
jamais zéro).

| Session | allocations thread audio | désallocations thread audio | dépassements | `max_sample_delta` | référence |
|---|---:|---:|---:|---:|---:|
| IIR v1 | 0 | 0 | 0 | 0,0253 | 0,0288 |
| IIR v2 (après swap) | 0 | 0 | 0 | **0,1079** | 0,0288 |
| Gain v1 | 0 | 0 | 0 | 0,0144 | 0,0288 |
| Gain v2 (après swap) | 0 | 0 | 0 | **0,6572** | 0,0288 |

Charge pendant la campagne : CPU 65 %, RAM libre 3,22 Go, horodatage
`2026-11-08T12:45:30Z`. Preuves : `evidence/mvp0b-dsp-iir.json`,
`evidence/mvp0b-dsp-gain.json`.

**Le coût de remplacement affiché (37 ms / 41 ms) est un chemin cache d'artefact**
(`build_ms` = 0) : ce n'est pas un rebuild réel. La campagne MVP-0a mesurait 289 ms avec
un vrai `rustc` ; les deux chiffres ne sont pas comparables et ne sont pas présentés comme
tels. Les discontinuités de 0,1079 et 0,6572 restent : **aucun crossfade n'est implémenté**
(lot L4-B du mandat, non commencé).

## 8. Limites honnêtes

1. **Aucune qualification temps réel** : pas de périphérique, pas de priorité RT, pas de
   `mlock`, pas de WCET, pas de mesure de jitter.
2. Le compteur instrumente les appels à l'allocateur Rust. Une désallocation interne à
   `System::dealloc`, un page fault ou une opération noyau ne sont pas comptés. Un compte
   non nul prouve une violation ; un compte nul ne prouve pas l'absence de toute latence.
3. La course de dernier `Arc` n'est **pas** reproduite de façon fiable en end-to-end (elle
   l'est de façon déterministe au niveau du slot). Ce n'est pas une preuve d'absence de
   course dans l'ancien code : c'est une preuve que la nouvelle API rend la course
   structurellement impossible.
4. Le thread de reclamation est global au processus et **ne peut pas être arrêté** par
   l'API. Le scénario « arrêt du worker de reclamation » du mandat est donc remplacé par
   un test de saturation de la file (200 bibliothèques) ; une panique du thread de
   reclamation laisserait la file non drainée et n'a pas de watchdog.
5. **Fermeture de l'hôte** : elle doit être faite par un thread de contrôle. Le motif
   recommandé (et utilisé par les tests) est `std::thread::scope` : le callback *emprunte*
   l'hôte et ne détient aucun `Arc<BlockHost>`, donc il ne peut pas être le dernier
   détenteur. Aucune API n'empêche un appelant de cloner l'`Arc` ; la règle est documentée
   et testée par construction dans les campagnes.
6. Aucun test multi-hôte ni multi-périphérique ; un seul écrivain sérialisé par `Mutex` de
   writer, deux écrivains concurrents restent interdits par conception (documenté).

## 9. Reproduire

```powershell
cd D:\App\Ravum
cargo test -p ravum-patch-host
cargo test -p ravum-patch-host --test rt_lifecycle -- --nocapture --test-threads=1
cargo run -q --bin ravum -- dsp fixtures/dsp/iir_v1.rs  --next fixtures/dsp/iir_v2.rs  --blocks 400 --json
cargo run -q --bin ravum -- dsp fixtures/dsp/gain_v1.rs --next fixtures/dsp/gain_v2.rs --blocks 400 --json
```
