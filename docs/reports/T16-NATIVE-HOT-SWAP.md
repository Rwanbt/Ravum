# T16 — hot swap par fonction sur le backend natif (2026-11-19)

> Gate **T16** (ADR-021/022/024, V1). Passe de `FAIL` à **`PARTIAL`**.
> Aucun `PROVED`, aucune garantie hard-RT.

## Objectif

Quand on modifie quelques lignes, ne recompiler **que la fonction modifiée**
et la remplacer **dans le programme qui tourne**, à un point sûr, sans
travail coûteux sur le thread temps réel (INV-05) et en refusant tout ce qui
est incompatible (INV-03).

## Articulation avec l'autre ligne de travail

`RFC-E11-HOT-RELOAD` et `RFC-E11-DSP-BLOCK` (autre session, sur `dev`)
rechargent **un module entier** compilé par `rustc` (processus piloté ou
`cdylib`), avec état déclaré, digest de schéma et `migrate`. Ce travail est
**complémentaire** : granularité **fonction**, en processus, sans `rustc`.
Il ne modifie aucun de leurs fichiers et ne redéfinit pas leurs règles
(`spec/language/reload.md`) ; l'alignement des deux voies (même schéma d'état,
mêmes codes de refus) reste à faire.

## Modification (`crates/ravum-lang-jit`)

1. **Table d'indirection** : tout appel passe par `Ctx::table` (`call_indirect`).
   Les 1 847 programmes différentiels de T15 repassent à l'identique.
2. **`hot::HotEngine`** (côté éditeur) : une case stable par nom de fonction ;
   `update` compare la forme canonique de chaque fonction (IR sérialisée, appels
   renommés en cases) et ne compile **que** celles qui changent (plus les
   appelants d'une fonction dont la signature change) dans un nouveau module
   JIT ; les autres adresses sont reprises telles quelles.
3. **Publication** : la nouvelle table part dans une boîte aux lettres à une
   case (`AtomicPtr`). Une version non encore prise est remplacée et libérée
   côté éditeur.
4. **`hot::RtRunner`** (côté temps réel) : `poll()` au point sûr (frontière de
   bloc) prend la version par un `swap` atomique. Aucune compilation,
   allocation, libération, verrou ni attente ; la version quittée va dans une
   case `retired` que l'éditeur libère (`collect`). Si cette case est pleine, le
   changement attend la frontière suivante.
5. **Retour arrière (HR-1.5)** : la version précédente est gardée ; si la
   nouvelle déclenche un trap, le runner y revient et rejoue l'échantillon.
6. **Refus fail-closed** : entrée absente ou de signature différente, IR
   invalide, échec de génération → rien n'est publié.
7. **Récupération** : un module JIT est libéré quand aucune version vivante
   (active, précédente, en attente) ne pointe dedans.

## Tests (`tests/hot_swap.rs`)

| Test | Type | Résultat |
|---|---|---|
| `only_the_edited_function_is_recompiled_and_swapped_at_the_safepoint` | positif | 1 fonction recompilée sur 22, ancienne version jusqu'au `poll`, nouvelle ensuite ; édition d'espaces → 0 recompilation |
| `an_incompatible_entry_is_refused_and_the_running_version_continues` | **négatif** | signature d'entrée changée et entrée supprimée refusées, rien de publié, sortie inchangée |
| `a_trapping_edit_rolls_back_to_the_previous_version` | **négatif** | `gain` divise par zéro → retour à la version précédente, sortie correcte, `division by zero` remonté |
| `real_time_thread_swaps_20_edits_without_allocating` | charge | thread « audio » (blocs de 64 échantillons, période 1,333 ms), 52 fonctions, 20 éditions ; allocateur compteur sur ce thread |

Mesures (`cargo test -p ravum-lang-jit --release --test hot_swap -- --nocapture`,
3 exécutions) :

| Mesure | Résultat |
|---|---|
| Allocations / libérations sur le thread temps réel | **0 / 0** (3/3 exécutions, et 0/0 en debug) |
| Édition → activation (source sauvée → premier bloc sur la nouvelle version) | **p50 ≈ 2,8–2,9 ms, p95 ≈ 2,8–2,9 ms, max ≈ 3,0 ms** |
| dont compilation de la fonction modifiée (`update`) | p50 ≈ 0,29–0,39 ms |
| Modules JIT vivants après 20 éditions | 3 (version active, précédente, dernière) |
| Sortie après chaque édition | égale à la valeur attendue (bit à bit) |
| Debug (vérificateur Cranelift actif) | p95 ≈ 13 ms, 0/0 |

Le délai d'activation contient : parse + check + abaissement du fichier entier
(~500 lignes), compilation d'une fonction, puis l'attente de la prochaine
frontière de bloc (≤ 1,33 ms).

## Limites

- Entrée **sans état** seulement : `fn(f32) -> f32` ou `fn(f64) -> f64`. L'état
  possédé par l'hôte et `migrate` exigent structs et références dans l'IR
  (IR v1.1) : non fait sur cette voie.
- Pas d'hôte audio réel (thread simulé avec `sleep`), pas de qualification
  hard-RT, pas de mesure de gigue.
- Non branché au CLI (`ravum-lang watch --backend jit` à faire) ; le front
  (parse/check) refait encore le fichier entier (T13 étape 2).
- Un seul thread exécute le code compilé (`RtRunner`) : l'argument de sûreté de
  la libération en dépend.
- Sites `unsafe` #7b, #8, #9 de `UNSAFE-INVENTORY.md` : `TESTED`, non audités.

## Commandes

```
cargo test -p ravum-lang-jit --test hot_swap -- --nocapture
cargo test -p ravum-lang-jit --release --test hot_swap -- --nocapture
cargo test -p ravum-lang-jit -p ravum-lang-ir -p ravum-lang-incr
cargo clippy -p ravum-lang-jit --all-targets -- -D warnings
python tools/unsafe_audit.py
cargo deny check licenses
```

## Contexte

Linux x86_64 (conteneur cloud, 4 cœurs, partagé), `rustc 1.95.0`,
Cranelift 0.116.1, branche `claude/trusting-cerf-4hhv5a`. État : `TESTED`.

## Suite proposée

1. IR v1.1 : structs/tableaux `Copy` et `&mut` vers un état possédé par l'hôte,
   pour l'entrée `process_sample(&mut State, f32) -> f32` de RFC-E11-DSP-BLOCK,
   avec le même digest de schéma (HR-1.1) et `migrate` (HR-1.7).
2. `ravum-lang watch --backend jit` (touche le CLI, crate partagé).
3. Brancher le `RtRunner` sur l'hôte audio existant (`ravum-patch-host`).
