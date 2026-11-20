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

## 7. Ajout du 2026-11-19 — bloc DSP avec état (IR v1.1)

Surface de RFC-E11-DSP-BLOCK : `struct State { z1: f32, z2: f32 }`,
`fn init() -> State`, `fn process_sample(state: &mut State, input: f32) -> f32`.
Le schéma est calculé par `ravum_lang_sema::reload::schema` : **même texte**
que `DSP_STATE_SCHEMA_V1_TEXT` de la voie `rustc` (asserté par le test).

- L'état est un tampon possédé par le `RtRunner` (alloué côté éditeur) ;
  `init` y écrit une fois, côté éditeur.
- `update` refuse un changement de digest d'état (`StateSchemaChanged`,
  HR-1.3), sauf `migrate` (HR-1.7, voir §9).
- Avant chaque appel, l'état est copié dans un tampon de secours préalloué ;
  sur trap, l'état est restauré avant le retour à la version précédente
  (HR-1.5).

| Test (`tests/dsp_hot.rs`) | Résultat |
|---|---|
| biquad, 10 000 échantillons | natif = interpréteur = Rust de référence, **bit à bit** |
| remplacement de `b0` à l'échantillon 1 000 | 1 fonction recompilée ; sortie = référence qui change de coefficient au même échantillon, **état conservé** |
| **négatif** : champ ajouté à `State` | `StateSchemaChanged`, rien de publié, sortie inchangée |
| **négatif** : nouvelle version qui écrit `z1` puis divise par zéro | état restauré, retour arrière, sortie = ancien filtre, `division by zero` remonté |
| thread temps réel, 20 éditions | **0 alloc / 0 libération**, édition→activation p50 ≈ 1,4 ms, p95 ≈ 1,4 ms (release), 3 modules vivants |

## 8. Ajout du 2026-11-19 — `ravum-lang watch --backend jit`

```
ravum-lang watch [PATH] --backend jit [--entry NAME]
```

Nouveau module `crates/ravum-lang-cli/src/commands/watch_jit.rs` ; retouches
minimales des fichiers partagés (`args.rs` : options `--backend rustc|jit` et
`--entry`, aide ; `commands/mod.rs` : une branche ; `Cargo.toml` : deux
dépendances). Le backend par défaut reste `rustc` ; `--backend jit` refuse
`--run`, `--reload` et `--release`, et `--entry` exige `--backend jit`.

À chaque sauvegarde du fichier (`src/main.rv`, sinon `src/lib.rv`, ou le
fichier donné ; scrutation toutes les 100 ms) : parse + check, abaissement en
IR v1.1, `HotEngine::update` (seules les fonctions modifiées sont compilées),
activation, puis exécution de l'entrée : `--entry NAME` (défaut `main`, sans
paramètre) ou `process_sample` si le fichier déclare un bloc DSP (piloté par un
signal de test déterministe de 4 096 échantillons, sans périphérique audio ;
l'état du bloc continue d'une version à l'autre).

Exemple réel (sortie du CLI) :

```
[ravum] jit: 3 functions compiled (front 0.8 ms, total 5.5 ms)
[ravum] check() -> value i64 37
[ravum] change detected
[ravum] jit: 1 recompiled (offset), 2 reused (front 0.6 ms, jit 1.4 ms)
[ravum] check() -> value i64 38
[ravum] change detected
[ravum] rejected by the checker; previous version kept
  src/main.rv:6:12: E-TYP-MISMATCH: return: expected `i64`, found `bool`
```

| Test (`crates/ravum-lang-cli/tests/watch_jit.rs`) | Résultat |
|---|---|
| édition d'une fonction | `1 recompiled (offset), 2 reused`, nouveau résultat |
| **négatif** : erreur de type | diagnostics `fichier:ligne:col`, version précédente gardée |
| **négatif** : construction hors IR (`.abs()`) | `not supported by the jit backend`, version précédente gardée |
| reprise après erreur | la sauvegarde suivante repart de la dernière bonne version |
| bloc DSP : coefficient modifié | `1 recompiled (gain)`, état conservé |
| **négatif** : `State` élargi (struct + `init`) | `refused: state schema changed`, version précédente gardée |
| **négatif** : signature de l'entrée changée | `refused: entry \`check\` changed signature` |
| analyse des options | `--backend jit` exclut `--run`/`--reload`/`--release` ; backend inconnu refusé ; `--entry` seul refusé ; défaut = `rustc` |

Non-régression (même branche, après l'ajout) : `cargo test -p ravum-lang-cli
--no-fail-fast` → **21 groupes, 167 passés / 0 échec / 3 ignorés** (dont les
tests `watch`, `hot_reload` et `dsp_block` de la voie `rustc`) ;
`cargo run -p ravum-conformance -- run` → **727 cas : 725 PASS / 0 FAIL /
2 SKIP** (sans `--verify-build` : `build_verified: false`, ce n'est pas une
preuve de build).

Limites : un seul fichier (pas de `mod x;`) ; pas de vrai périphérique
audio ; fin par Ctrl+C (aucun processus enfant à arrêter).

**Ajout (IR v1.2) — sortie `io::print*`.** `Inst::Print` écrit un littéral ou
le texte d'un scalaire (`&x.to_string()`, `x.to_string().as_str()`) ;
formatage `Display` de Rust côté hôte, donc le même texte que la voie `rustc`.
La sortie va dans un tampon de l'exécution (site `unsafe` #7d) et `watch`
l'affiche avant le résultat de l'entrée. Un module qui imprime est **refusé**
pour une entrée temps réel (INV-05). Autres chaînes (`String`, `&s`) :
`Unsupported`.

| Test | Résultat |
|---|---|
| `ravum-lang-jit/tests/print.rs` : 13 types × valeurs limites, `f64`/`f32` (±0, ±inf, NaN, `1e21`, `1e-7`, `0.30000000000000004`, `16777217.0`) | natif = interpréteur, octet par octet, stdout et stderr |
| sortie avant un trap, ordre du programme | conservés, identiques |
| **négatif** : entrée `fn(f32) -> f32` qui imprime | `Refusal::Invalid` (INV-05) |
| **négatif** : `io::println(&s)` avec `s: String` | `Unsupported` |
| `ravum-lang-cli/tests/jit_print_rustc.rs` : même programme par `ravum-lang run` (rustc) et par le JIT | **sortie identique octet par octet** (33 lignes) |
| `watch_jit` : `main` qui imprime, puis édition d'une fonction | `total=42` puis `total=100`, `1 recompiled (label)` |

## 9. Ajout du 2026-11-19 — `migrate` (HR-1.7), côté éditeur (décision V1-M1)

Un module dont `State` change est accepté quand sa fonction
`fn migrate(old: T) -> State` prend un `T` dont le schéma HR-1.1 est le schéma
courant (calculé par `reload::schema`, champ `Module::migrate_from`).
Protocole :

1. `update` construit la version, demande un instantané et rend
   `migration_pending: true` ; rien n'est publié.
2. Au point sûr suivant, le runner copie ses mots d'état dans une zone
   d'atomiques préallouée (4 096 mots au plus) : ni allocation, ni verrou.
3. `collect`/`collect_migration` (éditeur) exécute `migrate` compilé sur
   l'instantané, dans des tampons alloués par l'éditeur, puis publie la
   version avec ces tampons. Un trap dans `migrate` refuse le module.
4. À l'adoption, le runner échange ses tampons contre ceux de la version
   (`Box` déplacées) ; les anciens partent avec la version quittée et sont
   libérés côté éditeur. Un retour arrière (HR-1.5) rend à la version
   précédente **son** schéma et son état tel qu'à l'adoption.

| Test (`tests/migrate.rs`, `tests/replay.rs`, CLI `watch_jit.rs`) | Résultat |
|---|---|
| migration sans échantillon entre instantané et adoption | sortie = référence Rust **bit à bit** (état conservé, gain ×3) |
| **négatif** : `migrate` depuis un autre schéma | `StateSchemaChanged`, aucun instantané demandé |
| **négatif** : `migrate` qui divise par zéro | `Refusal::Invalid("migrate trapped: division by zero")`, rien de publié, bloc inchangé ; une migration correcte passe ensuite |
| **négatif** : nouvelle version qui déclenche un trap après migration | retour à l'ancienne version **et** à l'ancien état ; sortie = ancien filtre |
| thread temps réel | **0 allocation / 0 libération**, 1 instantané, 1 adoption |
| session réelle avec migration, rejouée | **21/21 identiques** ; écart mesuré instantané → adoption : 64 échantillons (1 bloc) ; **négatif** : instantané déplacé → sortie différente |
| `watch --backend jit` : `State` élargi avec `migrate` | `state migrated with \`migrate\` (HR-1.7)` |

Limite assumée (V1-M1) : les échantillons entre instantané et adoption ne
sont pas dans l'état migré (écart à HR-1.2, documenté).

## Suite proposée

1. ~~IR v1.1 et bloc DSP avec état~~ (fait, §7) ; ~~`migrate` (HR-1.7)~~ (fait, §9).
2. ~~`ravum-lang watch --backend jit`~~ (fait, §8).
3. Brancher le `RtRunner` sur l'hôte audio existant (`ravum-patch-host`).

## Note (2026-11-20) — échec isolé de deux tests `watch_jit`

Lors d'une exécution de la suite pendant que d'autres commandes `cargo`
tournaient en parallèle, `watch_jit_drives_a_dsp_block…` et
`watch_jit_recompiles_only_the_edited_function…` ont échoué en 0,49 s (pas un
dépassement du délai de 30 s : la sortie de l'enfant s'est fermée). Enquête :
30 exécutions du binaire de test, 6 en parallèle sous charge : **0 échec** ;
la suite relancée seule : verte. **Signature reproduite** en réécrivant
`target/debug/ravum-lang` pendant que les tests le lancent (mêmes lignes
d'échec, < 1 s). Cause la plus probable : un binaire réécrit par une
compilation concurrente — **non prouvée** pour l'exécution d'origine (sa
sortie d'erreur n'a pas été conservée). Le test distingue maintenant « délai
dépassé » de « sortie de l'enfant fermée » pour qu'un prochain cas soit
diagnostiqué. Aucun test désactivé ni affaibli.

## 10. Budget temps réel (2026-11-20)

**Pourquoi pas de « garantie hard-RT ».** Une garantie temps réel strict
exige deux choses : un coût **borné** de chaque appel (propriété du code,
démontrable ici) et un ordonnanceur qui exécute le thread audio à temps
(propriété de l'OS et du matériel : `SCHED_FIFO`/PREEMPT_RT, CPU isolé). La
seconde ne se prouve pas par un test et n'est pas sous le contrôle de Ravum.
Ce qui a été fait :

**Défaut trouvé et corrigé.** Le carburant d'un appel temps réel valait
10⁹ : une édition qui boucle bloquait le thread audio **2,34 s pour un
seul échantillon** (mesuré) avant le retour arrière. Une récursion sans
boucle n'était bornée que par la profondeur (arbre d'appels exponentiel).
- Chaque **entrée de fonction** brûle aussi 1 unité de carburant.
- `RT_FUEL = 100 000` unités par appel temps réel (les exécutions
  côté éditeur gardent 10⁹), réglable par l'hôte (`set_fuel_budget`).
- **Garde de pile** : `Ctx::stack_floor` (pointeur de pile de l'appelant −
  `STACK_BUDGET` = 1 MiB) comparé à l'entrée de chaque fonction : une
  récursion à grosses trames **trappe** au lieu de déborder la pile native
  (la limite de profondeur 512 ne bornait pas les octets). Le thread hôte
  doit avoir ≥ 1 MiB de pile libre plus son propre usage (2 MiB par défaut
  pour un thread Rust).

**Mesures (`tests/rt_budget.rs`, conteneur Linux non-RT, 4 vCPU, temps CPU
du thread via `CLOCK_THREAD_CPUTIME_ID`).**

| Cas | Résultat |
|---|---|
| édition qui boucle à l'infini | 1 appel = **≈ 0,26–0,45 ms CPU**, puis retour arrière (avant : 2,34 s) |
| récursion explosive sans boucle | **≈ 20 µs**, retour arrière |
| boucle DSP légitime (256 itérations/échantillon) | aucun trap |
| 20 000 blocs DSP de 64 échantillons, 50 hot swaps, ×8 exécutions | CPU par bloc : p50 ≈ 12 µs, p99,9 ≈ 41–58 µs, **max 174 µs** pour une échéance de **1,333 ms** ; **0** bloc dont le travail dépasse l'échéance. Murale : 2 dépassements sur 160 000 blocs, **tous préemptés** (CPU 93 et 127 µs pour 3,4 et 1,55 ms muraux) |
| récursion à trames de 32 KiB, 500 niveaux, thread de 2 MiB | trap (garde de pile), pas de crash |

Le test échoue si le **travail** d'un bloc dépasse son échéance, ou si un
dépassement mural survient **sans** préemption ; un dépassement dû à
l'ordonnanceur est compté et affiché, jamais masqué.

**Bug annexe trouvé.** L'interpréteur de référence recurse une fois par
appel IR : à la profondeur 512 il ne tenait plus dans la pile de 2 MiB d'un
thread de test (débordement de pile dans `differential.rs` selon les
features unifiées du build). Il s'exécute désormais sur son propre thread
(`INTERP_STACK` = 64 MiB réservés), et un test vérifie la profondeur
maximale sur un appelant à 1 MiB de pile.

Restent hors de Ravum : l'ordonnancement RT de l'OS, les allocations
natives du pilote audio, et un audit externe des sites `unsafe`.

**Pile du thread de rappel audio (complément).** Vérifié dans les sources de
`cpal` 0.15.3 : sous ALSA le rappel tourne sur un thread Rust créé par
`thread::Builder` sans taille explicite (2 MiB par défaut) ; sous CoreAudio
ou WASAPI c'est un thread du système, dont la pile peut être plus petite
(~512 KiB). Le budget de pile est donc **réglable par runner**
(`RtRunner::set_stack_budget`) et `CpalHost` utilise 256 KiB
(`CALLBACK_STACK_BUDGET`, aussi dans l'exemple `play`). Test :
`a_small_stack_budget_protects_a_small_callback_thread` — sur un thread de
512 KiB, du code qui demanderait 3,2 MiB trappe et revient à la version
précédente, sans débordement. `cpal` 0.15 ne demande aucune priorité temps
réel pour son thread.
