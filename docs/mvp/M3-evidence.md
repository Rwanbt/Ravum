# M3 — DSP : état réel (démonstrateur de bloc)

> Mesures du 2026-11-08, machine de référence (Windows 11, i7-13620H, 15,71 Go),
> `rustc 1.95.0`, builds en `opt-level=1`. **Ce n'est pas du temps réel dur** : pas de
> périphérique audio, pas de priorité de thread temps réel, pas de pilote. Toutes les
> valeurs sont locales et dépendantes de la charge.

## 1. Ce qui est implémenté

| Élément | Rôle | Fichier |
|---|---|---|
| ABI de bloc `v2` | `ravum_process_block(in, out, frames, state)` + manifeste | `crates/ravum-protocol/src/dsp.rs` |
| `DspState` `#[repr(C)]` | état **possédé par l'hôte** (2 × f32) | idem |
| `ArcSlot` | publication sans verrou, sans attente du bloc | `crates/ravum-patch-host/src/slot.rs` |
| `BlockHost` | mêmes portes que M1 (ABI, manifeste, fraîcheur, preuve) | `crates/ravum-patch-host/src/block.rs` |
| `ravum dsp` | session mesurée + remplacement à chaud optionnel | `crates/ravum-cli/src/dsp.rs` |
| Harnais | compteur d'allocations global, temps de bloc, dépassements, discontinuité | `crates/ravum-dsp-harness/` |

### Décision structurante : l'état appartient à l'hôte

Le module est une fonction pure `(bloc d'entrée, état) → bloc de sortie`. Conséquence
directe : un remplacement **compatible en disposition** conserve l'état sans aucune
migration de mémoire. ADR-022 reste ouvert pour les changements *incompatibles* — le
code refuse ceux-là (`SignatureMismatch`) au lieu de réinterpréter des octets.

## 2. Absence de verrou sur le chemin audio

`BlockHost::process_rt` fait un `ArcSlot::load` : un incrément atomique, **aucun
verrou, aucune allocation, aucune attente**. L'écrivain échange un pointeur et attend
seulement la fenêtre de deux atomiques pendant laquelle un lecteur pourrait avoir vu
l'ancien pointeur, jamais la durée d'un bloc.

La sémantique « remplacement à une frontière de bloc » vient de l'appelant : la boucle
audio charge le module **une fois par bloc** et le garde pour tout le bloc. Un bloc en
cours ne peut donc pas changer de version au milieu.

Preuves : `slot.rs` (2 tests, dont 4 lecteurs × 5 000 chargements concurrents de 200
publications, sans fuite : compteur de `Drop` exact) et `block_host.rs`
(`the_block_path_does_not_allocate`, `an_in_flight_block_keeps_the_retired_module_alive`).

## 3. Mesures réelles

Commande :

```powershell
ravum dsp fixtures/dsp/iir_v1.rs  --next fixtures/dsp/iir_v2.rs  --blocks 400 --json
ravum dsp fixtures/dsp/gain_v1.rs --next fixtures/dsp/gain_v2.rs --blocks 400 --json
```

Configuration : 400 blocs × 256 frames, 48 kHz, période dérivée = **5 333 µs**
(≈ 2,13 s d'audio par session). Sorties brutes : `evidence/m3-dsp-iir.json`,
`evidence/m3-dsp-gain.json`.

### 3.1 Filtre IIR (état conservé)

| Métrique | Avant | Après remplacement |
|---|---|---|
| temps/bloc p50 | 1 µs | 1 µs |
| temps/bloc p95 | 2 µs | 1 µs |
| temps/bloc max | 13 µs | 22 µs |
| dépassements de période | 0 | 0 |
| **allocations sur le thread audio** | **0** | **0** |
| `max_sample_delta` | 0,0253 | **0,1079** |
| référence sinusoïdale | 0,0288 | 0,0288 |
| coût du remplacement | — | chargement 2 ms, total 4 ms |

Lecture honnête :

- **l'état est conservé** : l'hôte le possède, le remplacement n'y touche pas (test
  `compatible_iir_swap_keeps_the_host_owned_state_and_the_signal_continuous`) ;
- **aucune allocation** sur les 400 blocs, avant comme après ;
- **mais le signal n'est pas continu** : 0,1079 contre une référence de 0,0288. Le
  changement de coefficient modifie la fonction de transfert ; le premier échantillon
  saute. C'est exactement le « clic de changement de paramètre » que ADR-024 traite
  par fondu. **Aucun crossfade n'est implémenté**, et je ne prétends pas le contraire.

### 3.2 Gain (0,5 → 2,0)

| Métrique | Avant | Après |
|---|---|---|
| temps/bloc p50 / p95 | 0 µs / 0 µs | 0 µs / 0 µs |
| allocations thread audio | 0 | 0 |
| `max_sample_delta` | 0,0144 | **0,6572** |
| référence sinusoïdale | 0,0288 | 0,0288 |
| coût du remplacement | — | chargement 2 ms, total 289 ms (dont build 287 ms) |

Un doublement de gain sans rampe produit un saut de 0,657 ≈ 0,75 × amplitude : c'est
un clic audible. **Il est mesuré, pas masqué.** Un fondu de sortie/entrée est le
travail suivant (ADR-024), pas une correction cosmétique du rapport.

### 3.3 Capacité, pas ordonnancement

Les sessions tournent **sans sommeil entre blocs** : elles mesurent une capacité, pas
un ordonnancement temps réel. 400 blocs × 1 µs de calcul pour 2,13 s d'audio donne un
facteur ≈ 5 000× — un plafond théorique sur cette machine, pas une garantie. Avec
`--period-sleep-us 5333` le harnais reproduit la cadence, mais un OS non temps réel ne
garantit pas l'échéance.

## 4. Tests exécutés

`crates/ravum-patch-host/tests/block_host.rs` (8 tests) :

| Test | Ce qu'il vérifie |
|---|---|
| `a_gain_patch_replaces_the_processing_at_a_block_boundary` | tout le bloc utilise une seule version |
| `an_incompatible_dsp_abi_is_refused_before_any_call` | ABI 1 refusée pour un hôte DSP (2) |
| `a_scalar_module_cannot_be_loaded_as_a_dsp_module` | un module `f64->f64` n'est pas un module DSP |
| `compatible_iir_swap_keeps_the_host_owned_state_and_the_signal_continuous` | état intact, pas de remise à zéro |
| `the_block_path_does_not_allocate` | 100 blocs, **0 allocation** sur le thread audio |
| `an_in_flight_block_keeps_the_retired_module_alive` | module retiré vivant pendant le bloc, libéré après |
| `republishing_the_active_module_is_refused` | `AlreadyActive` |
| `dsp_evidence_is_explicitly_bounded` | la preuve déclare son domaine d'un bloc |

Harnais : 5 tests (compteur par thread, absence d'allocation, discontinuité détectée,
période dérivée, enchaînement de sessions sans saut artificiel de phase).

## 5. Ce qui n'est PAS démontré

1. **Aucun hôte audio réel** : pas de CLAP/VST3, pas de pilote ASIO/WASAPI. C'est un
   jalon de qualification distinct (#59).
2. **Aucune garantie temps réel** : pas de priorité de thread, pas de `mlock`, pas de
   mesure de jitter d'ordonnancement, pas de WCET.
3. **Aucun crossfade** : les discontinuités mesurées en §3.1 et §3.2 sont réelles.
4. **Pas de migration d'état incompatible** : seul le cas « même disposition » est
   supporté, par construction.
5. **Allocations** : le compteur couvre tout le processus ; il est lu par thread, donc
   l'affirmation « 0 allocation sur le thread audio » est exacte, mais un allocateur
   alternatif (jemalloc, mimalloc) n'a pas été testé.
6. **Pas de test multi-thread du callback** : un seul thread audio, pas de contention
   entre plusieurs callbacks et plusieurs remplacements simultanés.

## 6. Reproduire

```powershell
cargo build --workspace
cargo test --workspace -- --test-threads=1
cargo run -q --bin ravum -- dsp fixtures/dsp/iir_v1.rs  --next fixtures/dsp/iir_v2.rs  --blocks 400 --json
cargo run -q --bin ravum -- dsp fixtures/dsp/gain_v1.rs --next fixtures/dsp/gain_v2.rs --blocks 400 --json
```
