# Qualification Linux — méthode et statut

> **Statut global : `PREPARED` / `NOT_YET_RUN`.**
> Ce document décrit une qualification **préparée** pour un runner GitHub Actions
> `ubuntu-latest`. Aucun run CI n'a encore été exécuté depuis cette session : le
> Lead pousse la branche puis déclenche le workflow. Tant que le run n'existe pas,
> aucun job n'est `PASS` et aucun résultat Linux n'est `PROVED`. Les seules
> preuves locales disponibles sont listées au §6.

## 1. Objet

Qualifier la chaîne **Ravum Core 1.0** sur Linux réel, alors que le poste de
développement ne peut plus fournir cette preuve :

- WSL local est hors service (`WslService` bloqué, `wsl --shutdown` se bloque,
  aucun accès administrateur pour réparer le service) ;
- le poste est Windows ; un run Git Bash local ne prouve pas Linux (noyau,
  chemins, processus, `rustc`/`cargo`, Node WASI).

La machine de qualification est donc le runner **GitHub Actions `ubuntu-latest`**,
indépendant du poste, recréé à chaque run. La preuve est liée au commit poussé et
au run CI ; elle n'est ni transférable à un autre commit, ni à une autre image.

## 2. Périmètre de preuve

Le workflow prouve, **si et seulement si** le run est vert :

1. le workspace compile et tous ses tests passent sur Linux
   (`cargo test --workspace --all-targets`) ;
2. le corpus déclaratif de conformance s'exécute sur Linux et produit un rapport
   JSON versionné ;
3. les exemples `examples/` passent le smoke réel de la CLI sur Linux
   (init/check/build/run/test/fmt/doc/cache, avec les mêmes attendus que le
   smoke PowerShell) ;
4. l'installateur POSIX, s'il est présent, installe un binaire exécutable sous un
   préfixe temporaire et ce binaire répond à `--version` ;
5. Miri exécute les tests de `ravum-lang-syntax` et `ravum-lang-sema` sur
   nightly, avec l'isolation désactivée ;
6. les tests `fuzz_*` marqués `#[ignore]` sont exécutés s'ils existent ;
7. `wasm32-wasip1` construit `examples/hello` et l'exécute via Node WASI, si la
   CLI expose `--target` (elle l'expose à ce commit).

Il ne prouve **pas** :

- le comportement Windows (le workflow `mvp.yml` reste la référence Windows) ;
- le temps réel, les performances, la consommation RAM/CPU ;
- une ABI stable, une release, une signature, un SBOM ;
- la correction sémantique de programmes arbitraires : la conformance est un
  corpus fini, Miri un contrôle dynamique borné, le smoke une surface de CLI ;
- la sûreté face à un adversaire : pas de fuzzing de sécurité exhaustif, pas de
  TSan (voir §5), pas de test réseau.

## 3. Fichiers livrés

| Fichier | Rôle |
|---|---|
| `.github/workflows/linux-qual.yml` | workflow `workflow_dispatch` + `push` ; jobs parallèles |
| `scripts/lang-smoke.sh` | port fidèle de `scripts/lang-smoke.ps1`, portable Linux/Git Bash |
| `scripts/qual-linux.sh` | driver de reproduction locale sur une machine Linux |
| `docs/qualification/LINUX-QUALIFICATION.md` | ce document |

Aucun fichier produit, exemple, spec ou crate n'est modifié par cette
qualification.

## 4. Jobs du workflow

Le workflow est déclenché par `workflow_dispatch` et par `push` sur `main`, `dev`,
`feat/**`, `fix/**`. Tous les jobs sont parallèles, `permissions: contents: read`.

| Job | Commande principale | Ce que le job prouve | Statut attendu |
|---|---|---|---|
| `workspace-tests` | `cargo test --workspace --all-targets -- --test-threads=2` | compilation + tests de tous les targets sur Linux | `UNKNOWN` jusqu'au run |
| `conformance` | `cargo build -p ravum-lang-cli --bin ravum-lang` puis `cargo run -p ravum-conformance -- run --json target/conformance-report.json` | corpus déclaratif exécuté ; rapport JSON uploadé même en échec | `UNKNOWN` jusqu'au run |
| `smoke` | `cargo build -p ravum-lang-cli --bin ravum-lang` puis `bash scripts/lang-smoke.sh --skip-build` | surface CLI réelle sur les exemples, mêmes attendus que Windows | `UNKNOWN` jusqu'au run |
| `install` | `bash scripts/install.sh --prefix "$RUNNER_TEMP/ravum-install"` puis `<prefix>/bin/ravum-lang --version` (step conditionné par `hashFiles('scripts/install.sh') != ''`) | contrat de l'installateur POSIX (build release `--locked`, manifeste, binaire exécutable) | `UNKNOWN` ; `NOT_RUN` si le script est absent |
| `miri` | `MIRIFLAGS=-Zmiri-disable-isolation RAVUM_FUZZ_ITERS=16 cargo +nightly miri test -p ravum-lang-syntax -p ravum-lang-sema --lib --tests` | contrôle dynamique Miri sur les crates du langage | `UNKNOWN` ; peut révéler des tests non compatibles Miri (voir §7) |
| `fuzz` | `cargo +nightly test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored --test-threads=1` (garde souple sur `tests/fuzz_*.rs`) | budget long déterministe du fuzz lexer/parser | `UNKNOWN` ; `NOT_RUN` explicite si le corpus disparaît |
| `wasm` | `cargo build --target wasm32-wasip1 examples/hello` puis `run --target wasm32-wasip1 examples/hello`, stdout `hello, ravum` | build et exécution WASI preview 1 via Node | `UNKNOWN` |
| `tsan` | *(désactivé : `if: false`)* | **NOT_APPLICABLE** | jamais exécuté |

### TSan — NOT_APPLICABLE (justifié)

ThreadSanitizer qualifie du code concurrent. Ravum Core 1.0 (`ravum-lang-syntax`,
`ravum-lang-sema`, `ravum-lang-codegen`, `ravum-lang-cli`) n'expose ni threads, ni
`unsafe`, ni FFI, ni runtime asynchrone ; aucun test du corpus ne crée de chemin
concurrent sur un état mutable partagé. Exécuter TSan ici produirait un signal
vert sans rapport avec le produit. Le job est conservé sous forme désactivée
(`if: false`) avec ce commentaire, pour qu'il ne soit pas confondu avec un oubli.
Il devra être réactivé quand un chemin concurrent réel (daemon multi-session,
overlays) sera qualifié avec ses propres tests et sa TCB.

## 5. `scripts/lang-smoke.sh` — port du smoke Windows

Port fidèle de `scripts/lang-smoke.ps1` :

- mêmes 28 cas : `--version`, `doctor`, `init` (+ fichiers, check, build, run),
  `hello`, `fib`, `calculator` (5 + 3), `stats` (fichier d'entrée), `dsp-gain-iir`,
  `package-demo/app`, `tests-demo`, `fmt --check`, et l'échec attendu
  `check --json` → `E-TYP-MISMATCH` (exit 1) ;
- mêmes attendus stdout et mêmes codes de sortie ;
- `set -euo pipefail`, répertoires temporaires nettoyés par `trap EXIT` ;
- binaire auto-détecté (`target/debug/ravum-lang` ou `.exe`) ou imposé par
  `--bin` / `RAVUM_LANG_BIN` ; `--skip-build` évite toute compilation ;
- binaire introuvable → `NOT_RUN` et exit **3** ; incohérence réelle → exit **1**.

Différence assumée avec PowerShell : le script bash est portable et a été
exécuté sous Git Bash (voir §6). Il ne dépend d'aucun outil hors `bash`, `cargo`
et le binaire.

## 6. Vérifications locales effectuées (avant CI)

Toutes les commandes ci-dessous ont été exécutées sur le poste Windows, dans
`D:\App\Ravum`. Elles ne remplacent pas le run Linux ; elles prouvent que les
fichiers livrés sont syntaxiquement valides et que le port du smoke est fidèle.

| Commande | Résultat brut |
|---|---|
| `python -c "import yaml; yaml.load(open('.github/workflows/linux-qual.yml', encoding='utf-8'), Loader=yaml.BaseLoader)"` | `YAML_OK`, 8 jobs listés : `workspace-tests, conformance, smoke, install, miri, fuzz, wasm, tsan` ; déclencheurs `workflow_dispatch` + `push` |
| `"C:\Program Files\Git\bin\bash.exe" -n scripts/lang-smoke.sh` | exit **0** |
| `"C:\Program Files\Git\bin\bash.exe" -n scripts/qual-linux.sh` | exit **0** |
| `cargo build -p ravum-lang-cli --bin ravum-lang` | exit **0** (warnings existants de la crate, hors périmètre) |
| `cargo metadata --locked --no-deps --format-version 1` | exit **0** : `Cargo.lock` est cohérent, le `--locked` de l'installateur ne devrait pas échouer pour un lock périmé |
| `bash scripts/lang-smoke.sh --skip-build` (Git Bash, Windows) | **28 PASS / 0 FAIL / 0 NOT_RUN**, exit **0** |
| `ravum-lang build --target wasm32-wasip1 examples/hello` | exit **0**, `examples/hello/.ravum/bin/hello.wasm` produit |
| `ravum-lang run --target wasm32-wasip1 examples/hello` | exit **0**, stdout `hello, ravum` (Node 26 local, Windows) |

Notes honnêtes :

- PyYAML interprète `on:` comme un booléen en mode YAML 1.1 ; la validation
  ci-dessus utilise `BaseLoader` pour garder la clé `on` et vérifier les
  déclencheurs. GitHub Actions utilise sa propre lecture YAML, pas PyYAML.
- Le smoke a été exécuté sous **Git Bash/Windows**, pas sous Linux : il prouve la
  fidélité du port, pas le comportement Linux. Le run `smoke` reste `UNKNOWN`
  jusqu'à l'exécution ubuntu-latest.
- Le test wasm local utilise Node 26 sur Windows ; la CI utilise Node 22 sur
  Ubuntu. Le chemin CLI est le même, mais la version de Node et le système
  diffèrent : le run CI reste la référence.
- L'installateur `scripts/install.sh` est POSIX et construit `target/release/
  ravum-lang` (sans `.exe`) : il n'a pas été exécuté sous Git Bash/Windows. Sa
  qualification est le job `install` sur Ubuntu.

## 7. Limites et risques résiduels

1. **Aucun run CI effectué.** Le workflow peut échouer pour des raisons Linux
   réelles (hypothèses de chemins, hooks Git, tests qui invoquent PowerShell ou
   `.exe`, différences de `rustc`). Ce serait une information, pas un bug du
   workflow ; il faudra corriger le code/test concerné ou documenter un `SKIP`
   explicite, jamais affaiblir un test pour obtenir du vert.
2. **Miri.** `-Zmiri-disable-isolation` autorise les accès fichiers, mais Miri
   n'exécute pas les sous-processus. Si un test de `ravum-lang-sema` ou
   `ravum-lang-syntax` lance `rustc`/un binaire, Miri échouera : le job sera
   rouge et devra être scopé explicitement (`--lib` ou `--skip`) avec une
   justification, pas masqué. `RAVUM_FUZZ_ITERS=16` borne seulement le test fuzz
   par défaut pour éviter un timeout Miri ; le test long reste hors Miri.
3. **Fuzz.** Le budget long par défaut est 20 000 itérations déterministes
   (`RAVUM_FUZZ_ITERS` peut l'augmenter). Ce n'est pas une couverture exhaustive,
   ni un corpus de sécurité, ni un test de la sémantique : seulement l'absence de
   panique/non-déterminisme observée sur ces entrées.
4. **wasm.** WASI preview 1 via Node.js n'est ni du navigateur, ni du GPU, ni du
   component model. `run --target` dépend de Node ≥ 22 et du flag expérimental
   `--experimental-wasi-unstable-preview1` ; la CI épingle Node 22. Si la CLI
   retire `--target`, le job enregistre `NOT_RUN` au lieu d'un faux PASS.
5. **Install.** Le job ne s'exécute que si `scripts/install.sh` est présent. Il
   prouve l'installation sous un préfixe temporaire et `--version` ; il ne prouve
   ni signature, ni provenance, ni SBOM, ni mise à jour.
6. **Conformance.** Le harness peut sortir 3 si un cas est `NOT_RUN`
   (environnement indisponible) ; ce n'est pas un `PASS`. Le rapport JSON est
   uploadé même en échec pour l'audit.
7. **TSan** n'est pas exécuté (voir §4). Aucune couverture concurrente n'est
   revendiquée.
8. **Caches et image.** Les durées et les versions d'outils dépendent de
   `ubuntu-latest` au moment du run ; le workflow épingle Rust 1.95.0 et Node 22,
   pas l'image. Aucune mesure de performance n'est produite.
9. **État Git.** Le workflow se déclenche sur `push` ; il ne peut pas être lancé
   depuis cette session. Le Lead pousse et lance `gh workflow run`.

## 8. Comment lancer la qualification

Côté Lead (seul écrivain Git) :

```bash
git push origin feat/lang-core-1.0-rc
gh workflow run linux-qual.yml --ref feat/lang-core-1.0-rc
gh run list --workflow=linux-qual.yml
gh run watch <run-id>
```

Reproduction locale sur une **vraie machine Linux** (pas Git Bash) :

```bash
bash scripts/qual-linux.sh                 # fmt + tests workspace + build + conformance + smoke
bash scripts/qual-linux.sh --quick         # fmt + build + smoke seulement
bash scripts/qual-linux.sh --miri --wasm   # ajoute Miri et la tentative wasm
```

`scripts/qual-linux.sh` sort 3 sur un hôte non-Linux : sous Windows, il ne sert
qu'à `bash -n` et au smoke portable, pas à la qualification.

## 9. Ce qui reste à faire après le premier run

1. Remplacer chaque `UNKNOWN` du §4 par le statut observé (avec l'URL du run et
   le SHA) ; ne jamais écrire `PASS` sans run vert.
2. Si `workspace-tests` échoue sur Linux, isoler le test et corriger le code ou
   documenter un `SKIP` de plateforme explicite (jamais supprimer).
3. Si `miri` échoue, scoper la commande avec preuve (test incompatible avec Miri)
   et le documenter ; ne pas passer `continue-on-error` en silence.
4. Si le fuzz long est trop lent sur le runner, fixer `RAVUM_FUZZ_ITERS` à une
   valeur bornée et documentée, ou déplacer le budget long dans un job nocturne.
5. Mettre à jour ce document avec les résultats bruts, les durées et les
   artefacts (`conformance-report-linux`).
