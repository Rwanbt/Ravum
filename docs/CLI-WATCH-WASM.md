# `ravum-lang` — `watch` / `dev` et cible WASM (`wasm32-wasip1`)

> **Statut : implémenté dans `ravum-lang` (branche `feat/lang-core-1.0-rc`), testé
> sur Windows avec rustc 1.95.0 et Node.js 26.13.0.** Aucun ADR n'est ratifié ;
> les limites ci-dessous sont explicites et aucune préservation d'état n'est
> revendiquée.

Périmètre de ce document : commande `watch` (alias `dev`) et option
`--target <triple>` de `build`/`run`. Les autres commandes sont décrites dans
`crates/ravum-lang-cli/README.md`.

## 1. `ravum-lang watch [PATH] [--run] [--release]` (alias `dev`)

```text
ravum-lang watch [PATH] [--run] [--release]
ravum-lang dev   [PATH] [--run] [--release]
```

`PATH` par défaut : répertoire courant. Un fichier `.rv` seul est accepté (son
répertoire parent est surveillé) ; un répertoire doit contenir `ravum.toml` ou un
unique `.rv` (mode fichier unique).

### Sémantique

1. Construction initiale (équivalent `build`, natif) puis affichage du résultat.
2. Boucle de **polling** toutes les **250 ms** (aucune dépendance externe) :
   - surveille récursivement les fichiers `.rv`, `ravum.toml` et `ravum.lock`
     sous `PATH` (mtime + taille) ;
   - ignore `.ravum/`, `target/`, `.git/`, les symlinks et tout fichier
     `*.ravum-tmp` ;
   - à chaque changement : `[ravum] change detected`, arrêt de l'enfant courant,
     reconstruction, puis relance si `--run` ;
   - une erreur de compilation affiche `[ravum] build failed (exit N)` et **la
     boucle continue** ;
   - une reconstruction réussie affiche `[ravum] build ok` et relance l'enfant
     si `--run`.
3. `--release` utilise `-C opt-level=3` (sinon `0`), comme `build`.
4. `Ctrl+C` : le processus installe un handler minimal (console Windows /
   SIGINT Unix) qui positionne un drapeau atomique ; la boucle s'arrête, tue et
   `wait` l'enfant, puis sort avec le code `0`.
5. `--max-rebuilds N` (option **interne**, destinée aux tests) : après `N`
   reconstructions déclenchées par des changements, la boucle s'arrête. Un
   enfant court encore vivant dispose de 1 s pour se terminer, puis il est tué.
   `N = 0` arrête après la construction initiale.

### Sorties exactes

```text
[ravum] watching <dir>
[ravum] build ok
[ravum] running <bin>
<sortie du programme>
[ravum] change detected
[ravum] build ok
[ravum] running <bin>
<sortie du programme>
```

`build failed` :

```text
[ravum] change detected
error[...]: ...        (diagnostics sur stderr)
[ravum] build failed (exit 1)
```

Les chemins affichés sont naturels (sans préfixe verbatim `\\?\`).

### Codes de sortie

| Code | Sens |
|---|---|
| `0` | arrêt normal (`Ctrl+C`, `--max-rebuilds`, aucun enfant fuité) |
| `1` | `PATH` invalide/illisible (la boucle ne démarre pas) |
| `2` | ligne de commande invalide (option inconnue, `--max-rebuilds` non entier) |
| `3` | `rustc` absent lors d'une reconstruction (affiché `build failed (exit 3)`, la boucle continue) |

### Limites honnêtes

- **Redémarrage, pas hot reload** : aucune préservation d'état, aucun transfert
  de tas, aucun swap de module. L'ancien processus est tué puis remplacé.
- Latence de détection = intervalle de polling (250 ms) + durée du build.
- Un changement effectué **pendant** un build est absorbé par le snapshot
  post-build ; il faut modifier à nouveau le fichier pour déclencher un rebuild.
- Seuls les fichiers sous `PATH` sont surveillés. Une dépendance locale située
  **hors** de `PATH` (`../util`) n'est pas surveillée : modifier la dépendance ne
  déclenche pas de rebuild.
- Les écritures de `ravum.lock` par le build sont absorbées (pas de boucle
  infinie), mais un outil externe qui réécrit `ravum.lock` déclenche un rebuild.
- `Ctrl+C` est propre en fonctionnement normal ; un `SIGKILL`/`TerminateProcess`
  ne peut exécuter aucun nettoyage (limite du système, documentée).
- Pas de redémarrage en mode `--target` WASM : `watch` construit toujours pour
  l'hôte.

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

- `watch` : rebuild observé après modification (sortie `v1` → `v2`), échec de
  compilation non fatal puis reprise, absence de processus enfant fuité
  (suppression de l'exécutable après arrêt — verrou Windows) ;
- WASM : build + run de `hello` (`hello, ravum`), run de `examples/fib` (`55`),
  transfert des arguments (`examples/calculator -- 5 + 3` → `8`), propagation du
  code de sortie et de stderr (`main -> Result` en `Err` → exit `1`,
  `error: boom`), triple inconnu (exit `2`), Node absent simulé via `RAVUM_NODE`
  (exit `3`, `E-CLI-WASM-RUNTIME`) ;
- les tests WASM sont explicitement `SKIP` (message sur stderr, pas d'échec
  fabriqué) si la cible rustc ou Node.js manque sur la machine.

## 4. Références

- `crates/ravum-lang-cli/README.md` — commandes, schéma JSON, codes du loader.
- `docs/architecture/RAVUM-CORE-DESIGN.md` §2 (CLI) et §3.8 (runtime).
- `spec/language/diagnostics.md` §8 (codes de sortie).
