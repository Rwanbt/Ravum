# `ravum-lang` — `watch` / `dev` et cible WASM (`wasm32-wasip1`)

> **Statut : implémenté dans `ravum-lang` (branche `feat/lang-core-1.0-rc`), testé
> sur Windows avec rustc 1.95.0 et Node.js 26.13.0.** Aucun ADR n'est ratifié ;
> les limites ci-dessous sont explicites et aucune préservation d'état n'est
> revendiquée.
>
> Depuis la correction fail-closed (novembre 2026), `watch --run` est un
> **redémarrage avec continuité de service** : l'ancienne version reste en vie
> pendant la compilation de la nouvelle, et un build en échec ne l'interrompt
> jamais. Ce n'est **pas** du hot reload et **aucun état** n'est préservé.

Périmètre de ce document : commande `watch` (alias `dev`) et option
`--target <triple>` de `build`/`run`. Les autres commandes sont décrites dans
`crates/ravum-lang-cli/README.md`.

## 1. `ravum-lang watch [PATH] [--run] [--release]` (alias `dev`)

```text
ravum-lang watch [PATH] [--run] [--release] [--max-rebuilds N]
ravum-lang dev   [PATH] [--run] [--release] [--max-rebuilds N]
```

`PATH` par défaut : répertoire courant. Un fichier `.rv` seul est accepté (son
répertoire parent est surveillé) ; un répertoire doit contenir `ravum.toml` ou un
unique `.rv` (mode fichier unique).

### Sémantique

1. Construction initiale (équivalent `build`, natif) puis affichage du résultat.
   Si les sources changent **pendant** cette construction, le résultat est
   périmé : il n'est pas activé et la construction est reprise.
2. Boucle de **polling** toutes les **250 ms** (aucune dépendance externe) :
   - surveille récursivement les fichiers `.rv`, `ravum.toml` et `ravum.lock`
     sous `PATH` (mtime + taille) ;
   - ignore `.ravum/`, `target/`, `.git/`, les symlinks et tout fichier
     `*.ravum-tmp`.
3. À chaque changement : `[ravum] change detected`, puis **construction avant
   toute action sur l'enfant** (l'ancienne version continue de tourner) :
   - **échec de compilation** : diagnostics sur stderr, `[ravum] build failed
     (exit N)`, l'ancienne version **reste en vie**, le polling continue. Aucun
     artefact périmé n'est activé ;
   - **sources modifiées pendant la compilation** : `[ravum] source changed
     during the build; discarding that result and rebuilding` — le résultat est
     jeté et reconstruit ; l'instantané le plus ancien n'est jamais activé. Deux
     gardes indépendants coopèrent : le pipeline relit les sources et compare des
     digests de **contenu** avant publication (`[ravum] build rejected as stale`);
     `watch` re-scanne en plus `(mtime, taille)` juste avant l'activation ;
   - **re-scan juste avant activation** : si une modification est encore arrivée
     entre le dernier scan et l'activation, `[ravum] the sources changed again
     before activation; ...` et le build n'est pas activé ;
   - **succès avec instantané stable** : `[ravum] build ok`, l'artefact est copié
     dans `.ravum/watch/<pid>/<nom>-<n>[.exe]` (taille vérifiée), la copie est
     lancée (`[ravum] running <copie>`), **puis seulement** l'ancien enfant est
     tué et `wait` (pas de zombie), et la copie remplacée est supprimée ;
   - **échec de lancement de la copie** : l'ancien enfant est conservé s'il est
     vivant ; sinon la dernière version saine connue est relancée. Rien n'est
     laissé silencieusement arrêté.
4. **Continuité de service, pas hot reload** : aucune préservation d'état, aucun
   transfert de tas, aucun swap de module. Pendant le basculement, l'ancien et le
   nouveau processus peuvent coexister quelques millisecondes (le nouveau doit
   démarrer avant que l'ancien ne soit tué) ; leurs sorties peuvent s'entrelacer.
5. `--release` utilise `-C opt-level=3` (sinon `0`), comme `build`.
6. `Ctrl+C` : le processus installe un handler minimal (console Windows / SIGINT
   Unix) qui positionne un drapeau atomique ; la boucle s'arrête, tue et `wait`
   l'enfant, supprime `.ravum/watch/<pid>/` puis sort avec le code `0`.
7. `--max-rebuilds N` (option **interne**, destinée aux tests) : après `N`
   **événements** de reconstruction, la boucle s'arrête. Un enfant court encore
   vivant dispose de 1 s pour se terminer, puis il est tué, récolté et le
   répertoire de staging est supprimé. `N = 0` arrête après la construction
   initiale (sans `--run`, aucun enfant n'est lancé).

### Pourquoi l'enfant exécute une copie

Sous Windows, une image exécutable en cours ne peut être ni écrasée ni
supprimée : `rustc`/`link` échoue avec `LNK1104: cannot open file` sur
`.ravum/bin/<nom>.exe` tant que ce processus tourne (mesuré ; la suppression du
fichier est également refusée). Construire **avant** de tuer l'ancienne version
impose donc que l'enfant n'exécute pas le chemin stable produit par le pipeline.
`watch` copie l'artefact fraîchement construit dans
`.ravum/watch/<pid>/<nom>-<n>[.exe]` et exécute cette copie ; le chemin stable
reste libre pour la reconstruction suivante. C'est aussi ce qui permet au
rename de publication atomique du pipeline (P0-B) de remplacer
`.ravum/bin/<nom>` alors qu'une version tourne : un `fs::rename` sur une image
Windows en cours échoue, mais il ne cible pas la copie exécutée. Si le pipeline
rend les artefacts immuables/par build, la copie deviendra redondante mais
restera correcte.

### Sorties exactes

```text
[ravum] watching <dir>
[ravum] build ok
[ravum] running <copie .ravum/watch/<pid>/<nom>-<n>[.exe]>
<sortie du programme>
[ravum] change detected
[ravum] build ok
[ravum] running <copie .ravum/watch/<pid>/<nom>-<n+1>[.exe]>
<sortie du programme>
```

`build failed` (l'ancienne version continue de tourner) :

```text
[ravum] change detected
error[...]: ...        (diagnostics sur stderr)
[ravum] build failed (exit 1)
```

résultat périmé :

```text
[ravum] change detected
[ravum] source changed during the build; discarding that result and rebuilding
[ravum] build ok
```

Les chemins affichés sont naturels (sans préfixe verbatim `\\?\`).

### Codes de sortie

| Code | Sens |
|---|---|
| `0` | arrêt normal (`Ctrl+C`, `--max-rebuilds`, aucun enfant fuité, staging supprimé) |
| `1` | `PATH` invalide/illisible (la boucle ne démarre pas) |
| `2` | ligne de commande invalide (option inconnue, `--max-rebuilds` non entier) |
| `3` | `rustc` absent lors d'une reconstruction (affiché `build failed (exit 3)`, la boucle continue) |

### Limites honnêtes

- **Redémarrage, pas hot reload** : aucune préservation d'état, aucun transfert
  de tas, aucun swap de module. L'ancien processus est remplacé, pas mis à jour.
- Latence de détection = intervalle de polling (250 ms) + durée du build.
- Le **déclencheur** de reconstruction et le re-scan avant activation reposent
  sur `(mtime, taille)` : une modification qui conserve **à la fois** mtime et
  taille ne déclenche pas de rebuild. Pendant un build, la garde de publication
  du pipeline compare en revanche des digests de **contenu** et rejette
  l'artefact périmé même dans ce cas (avant publication) ; `watch` re-scanne
  ensuite les fichiers.
- Un changement effectué **pendant** un build est rejeté puis reconstruit (il
  n'est plus absorbé) : jusqu'à 16 tentatives par lot ; si les sources changent
  encore, un nouveau lot reprend immédiatement afin de construire la dernière
  version dès que les sources se stabilisent.
- Seuls les fichiers sous `PATH` sont surveillés. Une dépendance locale située
  **hors** de `PATH` (`../util`) n'est pas surveillée : modifier la dépendance ne
  déclenche pas de rebuild.
- Les écritures de `ravum.lock` par le build sont absorbées (pas de boucle
  infinie), mais un outil externe qui réécrit `ravum.lock` déclenche un rebuild.
- `Ctrl+C` est propre en fonctionnement normal ; un `SIGKILL`/`TerminateProcess`
  ne peut exécuter aucun nettoyage (limite du système). Il peut alors rester un
  enfant et un répertoire `.ravum/watch/<pid>/` (petites copies) : un prochain
  `watch` de même PID le supprime au démarrage, et `cache clean` supprime
  `.ravum/`. Les processus orphelins ne peuvent pas être récoltés dans ce cas.
- Pas de redémarrage en mode `--target` WASM : `watch` construit toujours pour
  l'hôte.
- `stdin` de `watch` est hérité par l'enfant ; `stdout`/`stderr` des enfants sont
  hérités et entrelacés avec les lignes `[ravum]`.

## 2. `--target <triple>` sur `build` et `run`

```text
ravum-lang build [PATH] --target wasm32-wasip1
ravum-lang run   [PATH] --target wasm32-wasip1 [-- ARGS...]
```

Seul le triple canonique `wasm32-wasip1` est accepté pour l'instant ; sans
option, la cible est l'hôte. Tout autre triple est une erreur d'usage (exit `2`)
— jamais un build hôte silencieux.

### Prérequis

- rustc avec la cible installée :
  `rustup target add wasm32-wasip1` ;
- Node.js 22+ (testé avec 26.13.0). L'exécutable peut être remplacé par
  `RAVUM_NODE` ;
- le code généré est le même que pour l'hôte (prélude `std` uniquement, pas de
  dépendance externe, pas de `unsafe`).

### `build --target wasm32-wasip1`

- invoque
  `rustc --edition 2021 -C overflow-checks=on -C debuginfo=2 -C opt-level=0|3 --target wasm32-wasip1 -o <out> <généré>` ;
- écrit `.ravum/gen/main.rs` (ou `--emit-rust FILE`) et
  `.ravum/bin/<name>.wasm` ;
- cible réservée aux crates binaires : un package bibliothèque est refusé avec
  un message clair (exit `1`) ;
- si la cible rustc manque, l'erreur brute de `rustc` est remontée (exit `1`) ;
  `--emit-ir` reste utilisable.

### `run --target wasm32-wasip1`

1. vérifie la présence de Node.js **avant** de compiler ; absent/inutilisable →
   `E-CLI-WASM-RUNTIME` sur stderr, exit `3` ;
2. construit le `.wasm` comme ci-dessus ;
3. génère un lanceur Node.js CommonJS à côté de l'artefact :
   `.ravum/bin/<name>-wasi.cjs` ;
4. exécute
   `node --experimental-wasi-unstable-preview1 <launcher> <wasm> [ARGS...]`.

Le lanceur crée `new WASI({ version: "preview1", args, env, returnOnExit: true })`
et transmet `args` (nom du module + arguments), l'environnement, stdin/stdout/
stderr et le code de sortie du programme. Node affiche sur stderr son
avertissement expérimental WASI ; cela ne change pas stdout ni le code de sortie.

Le code de sortie du lanceur Node est mappé avec la même fonction honnête que
`run` natif : un code négatif (exception Windows) donne `1` avec le code exact,
un code `> 255` est tronqué aux 8 bits (sauf s'il deviendrait `0`, alors `1`).

Exemples vérifiés :

```text
$ ravum-lang build examples/hello --target wasm32-wasip1
generated: D:\App\Ravum\examples\hello\.ravum\gen\main.rs
built: D:\App\Ravum\examples\hello\.ravum\bin\hello.wasm
$ ravum-lang run examples/hello --target wasm32-wasip1
hello, ravum
$ ravum-lang run examples/fib --target wasm32-wasip1
0 1 1 2 3 5 8 13 21 34 55        (une valeur par ligne, 55 inclus)
```

### Codes de sortie

| Code | Sens |
|---|---|
| `0` | build/run réussis (le code du programme WASM est propagé par `run`) |
| `1` | compilation Ravum, génération ou `rustc --target` en échec ; cible WASM demandée pour une bibliothèque |
| `2` | triple inconnu, option inconnue |
| `3` | Node.js absent/inutilisable (`E-CLI-WASM-RUNTIME`), rustc absent |

### Limites WASI preview1

- **Pas de préouverture de répertoire hôte** : le lanceur n'active aucun
  *preopen*. Les appels `std::fs` échouent donc à l'exécution (« file system
  access is not available » côté WASI) ; stdin/stdout/stderr, les arguments et
  l'environnement fonctionnent. Une option de preopen explicite pourra être
  ajoutée plus tard, avec validation de chemin.
- Pas de threads, pas de `net`, pas d'async, pas de FFI : ces fonctionnalités
  sont déjà hors Core 1.0 (`E-UNSUPPORTED-*`) ; la cible WASM ne les active pas.
- Le panic hook du prélude appelle `std::process::exit(1)`, supporté par
  `wasm32-wasip1` ; un panic se traduit donc par `panic: <message>` sur stderr et
  un code `1`.
- `wasm32-wasip1` est une cible WASI **preview 1** ; preview 2/3 ne sont pas
  supportées par cette commande.
- Le `.wasm` n'est pas signé et le lanceur Node n'est pas un bac à sable
  complet : n'exécutez que du code Ravum de confiance.

## 3. Tests automatisés

`cargo test -p ravum-lang-cli -- --test-threads=1` couvre :

- `watch` (`tests/cli.rs`, inchangés) : rebuild observé après modification
  (sortie `v1` → `v2`), échec de compilation non fatal puis reprise, absence de
  processus enfant fuité (suppression de l'exécutable après arrêt — verrou
  Windows) ;
- `watch` fail-closed (`tests/watch_continuity.rs`, ajouté) :
  - un build invalide **ne** redémarre **rien** et l'ancien enfant tient encore
    sa copie exécutable sur Windows (preuve de continuité) ;
  - un changement survenu pendant un build lent (`RAVUM_RUSTC` factice qui dort
    puis appelle le vrai `rustc`) est rejeté (`discarding that result`), la
    version intermédiaire n'est jamais lancée, la version finale l'est ;
  - après `--max-rebuilds`, `.ravum/watch/` a disparu (aucun répertoire
    orphelin) ; `--max-rebuilds 0` sans `--run` ne lance rien et ne crée rien ;
- WASM : build + run de `hello` (`hello, ravum`), run de `examples/fib` (`55`),
  transfert des arguments (`examples/calculator -- 5 + 3` → `8`), propagation du
  code de sortie et de stderr (`main -> Result` en `Err` → exit `1`,
  `error: boom`), triple inconnu (exit `2`), Node absent simulé via `RAVUM_NODE`
  (exit `3`, `E-CLI-WASM-RUNTIME`) ;
- les tests WASM sont explicitement `SKIP` (message sur stderr, pas d'échec
  fabriqué) si la cible rustc ou Node.js manque sur la machine.

Le handler `Ctrl+C` n'est pas testé de bout en bout : envoyer un vrai
`CTRL_C_EVENT` à un enfant Windows depuis un test d'intégration n'est pas
portable. Le chemin de sortie qu'il emprunte (tuer/`wait`, supprimer le staging,
exit `0`) est identique à celui de `--max-rebuilds`, qui est testé.

## 4. Références

- `crates/ravum-lang-cli/README.md` — commandes, schéma JSON, codes du loader.
- `docs/architecture/RAVUM-CORE-DESIGN.md` §2 (CLI) et §3.8 (runtime).
- `spec/language/diagnostics.md` §8 (codes de sortie).
