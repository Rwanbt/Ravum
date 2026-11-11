# MVP-0b / L4-B — Crossfade de remplacement DSP (sans clic de transition)

> **Portée** : suppression des discontinuités mesurées au remplacement d'un module DSP
> (M3 : 0,108 pour un changement de coefficient IIR, 0,657 pour un doublement de gain,
> contre une référence sinusoïdale de 0,029). Ce document décrit la solution implémentée,
> les mesures avant/après et ce qui **n'est pas** démontré. Aucune qualification temps
> réel dur n'est revendiquée : simulateur de blocs, pas de périphérique audio.
>
> Machine : Windows 11 Pro 26100, i7-13620H (10 cœurs / 16 threads), 15,71 Go RAM,
> `rustc 1.95.0`, fixtures DSP en `opt-level=1`. Charge CPU 34 % et RAM libre 3,43 Go au
> moment de la campagne finale (d'autres agents tournaient : les valeurs sont locales).

## 1. Problème mesuré (MVP-0a)

Un remplacement de module change la fonction de transfert d'un seul coup, à la frontière
de bloc. Mesuré par `max_sample_delta` (plus grand écart entre échantillons consécutifs de
la sortie) sur le signal continu avant/après :

| Session | `max_sample_delta` MVP-0a | Référence sinusoïdale |
|---|---:|---:|
| IIR, coefficient 0,1 → 0,5 | **0,1079** | 0,0288 |
| Gain 0,5 → 2,0 | **0,6572** | 0,0288 |

Ces chiffres sont des discontinuités de signal, pas des XRUN. Le rapport M3 les publiait
déjà sans les masquer ; L4-B les traite.

## 2. Solution implémentée : crossfade de sortie borné, sans allocation

À l'activation, si un crossfade est armé (`crossfade_frames > 0`), le module remplacé est
**conservé vivant dans le payload publié** et le callback rend :

```text
sortie[n] = ancien[n] * (1 - w[n]) + nouveau[n] * w[n]
w[n] : rampe linéaire 0 -> 1 sur `crossfade_frames` échantillons
```

Points d'implémentation qui font la différence entre « ça marche » et « ça déplace le
clic » :

1. **L'ancien module garde son propre état continu.** Un premier essai passait à
   l'ancien module une copie de l'état courant à chaque bloc. Résultat mesuré : la
   discontinuité IIR **augmentait** (0,112) parce que l'ancien filtre recevait chaque bloc
   l'état produit par le nouveau filtre — son état sautait à chaque frontière. L'état du
   fade est donc conservé **par thread audio** dans un `thread_local` `Cell<Option<FadeState>>`
   (`Copy`, initialisation constante, aucune allocation, aucun destructeur).
2. **Aucune allocation sur le callback.** Le buffer de sortie de l'ancien module est un
   tableau de pile `[f32; MAX_FADE_FRAMES = 512]` ; le fade peut être plus long que 512
   échantillons, il est alors réparti sur plusieurs blocs. Le seul état partagé est un
   compteur atomique d'observabilité (`fade_rendered`).
3. **Identité de publication process-wide.** Chaque publication reçoit un `fade_epoch`
   unique (compteur atomique global). Sans cela, deux hôtes publiant tous deux la
   « génération 2 » réutilisaient un état de fade périmé d'un autre hôte et le crossfade
   était silencieusement sauté (défaut observé en test, corrigé).
4. **Sûreté multi-lecteurs.** Le fade est entièrement par thread : plusieurs threads
   audio partageant un même hôte rendent chacun leur propre rampe, sans donnée partagée
   mutable. Aucun `UnsafeCell`, aucune course. Un hôte réel a un seul callback ; ce cas
   n'est donc pas un compromis, c'est une propriété supplémentaire.
5. **Sémantique par défaut inchangée.** `BlockHost::bootstrap` arme un crossfade de 0 :
   le remplacement immédiat (MVP-0a) reste le comportement par défaut et les tests
   existants le vérifient. Le crossfade est opt-in (`bootstrap_with_crossfade`, CLI
   `ravum dsp … --crossfade-frames N`).

### Fichiers

| Fichier | Changement |
|---|---|
| `crates/ravum-patch-host/src/block.rs` | payload `BlockProgram` (module courant + module en fondu + epoch), état de fade par thread, mixage, `bootstrap_with_crossfade`, `active_fade_remaining` |
| `crates/ravum-patch-host/src/lib.rs` | exports `BlockProgram`, `MAX_FADE_FRAMES`, `DEFAULT_CROSSFADE_FRAMES` |
| `crates/ravum-patch-host/tests/crossfade.rs` | 5 tests (clic gain, clic IIR, 0 allocation, double swap pendant un fondu, défaut sans fondu) |
| `crates/ravum-patch-host/tests/rt_lifecycle.rs` | adapter au payload `BlockProgram` (les garanties temps réel restent vérifiées) |
| `crates/ravum-cli/src/dsp.rs` | option `--crossfade-frames`, champ `config.crossfade_frames` dans la sortie JSON |

## 3. Résultats mesurés

### 3.1 Campagne CLI (400 blocs × 256 frames, 48 kHz, `--crossfade-frames 256`)

| Session | `max_sample_delta` sans fondu (MVP-0a) | avec fondu (MVP-0b) | Référence | Allocations / désallocations thread audio |
|---|---:|---:|---:|---|
| IIR 0,1 → 0,5 | 0,1079 | **0,0294** | 0,0288 | 0 / 0 |
| Gain 0,5 → 2,0 | 0,6572 | **0,0576** | 0,0288 | 0 / 0 |

Lecture honnête :

- **IIR** : 0,0294 contre une référence de 0,0288 ; la discontinuité ajoutée par le
  remplacement devient indistinguable de la pente normale du signal dans cette métrique.
- **Gain** : 0,0576 = exactement `2 × référence`. C'est le minimum théorique pour une
  rampe de gain qui atteint 2,0 : la sortie vaut `g(t)·x(t)` avec `g → 2`, donc le plus
  grand écart entre échantillons est au plus `2 ×` celui de l'entrée plus le terme de
  rampe (`|x|·Δg ≈ 0,5 · 1,5/256 ≈ 0,003`). On ne peut pas descendre sous ce plancher
  sans ralentir la rampe ; le crossfade a supprimé le saut, pas la pente.
- **0 allocation et 0 désallocation** sur le thread audio pendant les fondus, mesuré par
  `allocations_on_audio_thread` / `deallocations_on_audio_thread`.

Preuves brutes : `evidence/mvp0b-dsp-crossfade-iir.json`,
`evidence/mvp0b-dsp-crossfade-gain.json` (et `evidence/mvp0b-dsp-iir.json`,
`mvp0b-dsp-gain.json` pour la référence sans fondu du même cycle).

### 3.2 Campagne de test instrumentée

| Test | Mesure |
|---|---|
| `a_crossfaded_gain_swap_removes_the_click` | pire cas sur 3 phases : 0,6426 → **0,0576** |
| `a_crossfaded_iir_swap_removes_the_click` | pire cas sur 3 phases : 0,0634 → **0,0290** |
| `the_crossfade_path_does_not_allocate_or_free_on_the_audio_thread` | 8 remplacements pendant une boucle audio : **0 allocation, 0 désallocation** |
| `a_second_swap_during_a_fade_keeps_the_host_sound_and_settles` | second remplacement pendant le fondu : sortie finie, fondu terminé, 2 activations |
| `no_crossfade_is_the_default_and_keeps_the_immediate_switch` | défaut : `active_fade_remaining == 0`, sémantique MVP-0a conservée |

Le test échantillonne trois phases de signal (37, 80, 400 blocs) parce que l'amplitude du
clic dépend de l'endroit où tombe la frontière de bloc dans la sinusoïde : mesurer une
seule phase aurait rendu le test instable.

## 4. Alternatives étudiées et rejetées

| Alternative | Raison du rejet |
|---|---|
| Rampe de paramètres **dans le module** | Le module devrait exposer des paramètres nommés et une sémantique de rampe ; c'est le bon modèle à long terme (ABI de paramètres), mais cela ne couvre pas un remplacement de **code** (algorithmes différents) et imposerait un nouvel ABI dans ce lot. Conservé comme piste ADR. |
| État de fondu partagé dans le payload (`UnsafeCell`) | Course dès que deux callbacks partagent l'hôte ; rejeté pour la sûreté. L'état par thread est plus simple et strictement plus sûr. |
| Recopier l'état de l'ancien module à chaque bloc | Mesuré : **aggrave** le clic (0,112) au lieu de le réduire. Rejeté par la mesure, pas par intuition. |
| Fondu d'entrée/sortie par simple gain sur la sortie totale | Ne traite pas le changement de fonction de transfert (IIR), seulement l'amplitude. |
| Attendre la fin du fondu avant de publier | Bloque le thread d'intégration sur un état audio ; inutile et fragile. |
| Fondu adaptatif à la détection de discontinuité | Complexité de décision par bloc et risque de faux positifs ; non nécessaire pour le gain mesuré. |

## 5. Limites honnêtes

1. **Simulateur** : pas de périphérique audio, pas de pilote, pas de priorité temps réel,
   pas de mesure de jitter ni de WCALL. Aucune qualification hard-RT.
2. **Crossfade de sortie, pas migration d'état.** L'ancien module reçoit un état continu
   pendant le fondu, mais sa trajectoire d'état est abandonnée à la fin ; l'état de l'hôte
   appartient au nouveau module. Une disposition d'état incompatible reste **refusée** par
   la porte ABI (ADR-022 reste ouvert) ; ce n'est pas une migration.
3. **Pas de seuil psychoacoustique** : 0,029 / 0,058 sont des métriques de signal. Le
   seuil d'audibilité dépend du contenu, du niveau, de la fréquence et du système
   d'écoute ; aucun seuil universel n'est revendiqué.
4. **Le crossfade allonge de `fade_frames` la durée pendant laquelle deux modules sont
   chargés** (le module remplacé reste vivant dans le payload jusqu'au remplacement
   suivant). Coût mémoire borné et mesuré, mais réel.
5. **Un second remplacement pendant un fondu** redémarre un fondu depuis le module
   courant : la discontinuité résiduelle est bornée par la fraction de fondu restante,
   pas nulle. Le test vérifie seulement l'absence de sortie non finie et la convergence.
6. **Longueur de fondu fixe** (paramètre d'armement), pas adaptative au type de
   changement. `--crossfade-frames` reste un choix explicite.
7. Les compteurs d'allocations/désallocations instrumentent l'allocateur Rust ; une
   opération noyau ou un page fault ne sont pas comptés.

## 6. Reproduire

```powershell
cd D:\App\Ravum
cargo test -p ravum-patch-host --test crossfade -- --nocapture --test-threads=1
cargo run -q --bin ravum -- dsp fixtures/dsp/iir_v1.rs  --next fixtures/dsp/iir_v2.rs  --blocks 400 --crossfade-frames 256 --json
cargo run -q --bin ravum -- dsp fixtures/dsp/gain_v1.rs --next fixtures/dsp/gain_v2.rs --blocks 400 --crossfade-frames 256 --json
# comparaison sans fondu : omettre --crossfade-frames
```
