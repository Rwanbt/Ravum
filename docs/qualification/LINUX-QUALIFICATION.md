# Qualification Linux — méthode et statut

> **Statut global : `PASS` sur le dernier rejeu
> [43562818570](https://github.com/Rwanbt/Ravum/actions/runs/43562818570)
> (SHA `878e212`, 2026-11-14, branche `goal/production-ready`) — voir §12.**
> Le run de référence antérieur reste
> [43026622427](https://github.com/Rwanbt/Ravum/actions/runs/43026622427)
> (SHA `c67f17e`, 2026-11-11, §11) : 8 jobs à résultat explicite sur
> `ubuntu-latest` (`workspace-tests`, `conformance`, `smoke`, `install`, `miri`,
> `fuzz`, `wasm` **PASS** ; `tsan` **SKIP `NOT_APPLICABLE`**). Le premier run
> vert historique ([42675213985](https://github.com/Rwanbt/Ravum/actions/runs/42675213985),
> SHA `78c8f72`) reste au §10. WSL local reste hors service et n'est **pas** une
> preuve ; la qualification repose sur une machine indépendante.
>
> **Deux échecs ont précédé ce run** (runs `43020732371` et `43023874117`) : deux
> défauts de **nos propres tests**, révélés par Linux, pas des défauts du
> langage. Causes et correctifs : §11.1.

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
2. le corpus déclaratif de conformance s'exécute sur Linux avec
   `--verify-build` — la règle `check ⇒ build` (tout programme accepté par le
   `check` doit aussi être accepté par le `build`) — et le job **échoue** si le
   rapport ne porte pas `build_verified: true` ; il produit un rapport JSON
   versionné ;
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

- le comportement Windows : ce workflow ne l'exécute pas ; `mvp.yml` couvre
  Windows (`windows-latest`) pour le build, les tests et les démos — pas la
  conformance, l'installateur, la reproductibilité ni le SBOM
  ([`PLATFORMS-MATRIX.md`](PLATFORMS-MATRIX.md) §3.2) ;
- le temps réel, les performances, la consommation RAM/CPU ;
- une ABI stable, une release, une signature, un SBOM ;
- la correction sémantique de programmes arbitraires : la conformance est un
  corpus fini, Miri un contrôle dynamique borné, le smoke une surface de CLI ;
- la sûreté face à un adversaire : pas de fuzzing de sécurité exhaustif, pas de
  TSan (voir §5), pas de test réseau.

## 3. Fichiers livrés

| Fichier | Rôle |
|---|---|
| `.github/workflows/linux-qual.yml` | workflow `workflow_dispatch` + `push` ; jobs parallèles ; job `conformance` avec `--verify-build` et assertion bloquante `build_verified: true` |
| `docs/qualification/CONFORMANCE-BUILD-CONTRACT.md` | contrat explicite `check ⇒ build` : ce que `--verify-build` mesure, et ce qu'il ne prouve pas |
| `scripts/lang-smoke.sh` | port fidèle de `scripts/lang-smoke.ps1`, portable Linux/Git Bash |
| `scripts/qual-linux.sh` | driver de reproduction locale sur une machine Linux |
| `docs/qualification/LINUX-QUALIFICATION.md` | ce document |

Aucun fichier produit, exemple, spec ou crate n'est modifié par cette
qualification.

## 4. Jobs du workflow

Le workflow est déclenché par `workflow_dispatch` et par `push` sur `main`, `dev`,
`feat/**`, `fix/**`. Tous les jobs sont parallèles, `permissions: contents: read`.

| Job | Commande principale | Ce que le job prouve | Statut observé (run 43026622427) |
|---|---|---|---|
| `workspace-tests` | `cargo test --workspace --all-targets -- --test-threads=2` | compilation + tests de tous les targets sur Linux | **PASS** (durée ≈ 15 min 08 s d'après l'API) |
| `conformance` | `cargo build -p ravum-lang-cli --bin ravum-lang` puis `cargo run -p ravum-conformance -- run --verify-build --json target/conformance-report.json`, puis assertion `build_verified: true` | corpus déclaratif exécuté et règle `check ⇒ build` mesurée ; rapport JSON uploadé même en échec | **PASS** (étapes « Run conformance corpus (check ⇒ build contract) » et « Assert build_verified in the report ») |
| `smoke` | `cargo build -p ravum-lang-cli --bin ravum-lang` puis `bash scripts/lang-smoke.sh --skip-build` + balayages stdlib/CLI/JSON | surface CLI réelle sur les exemples, mêmes attendus que Windows | **PASS** (43 assertions, §11) |
| `install` | `bash scripts/install.sh --prefix "$RUNNER_TEMP/ravum-install"` puis `<prefix>/bin/ravum-lang --version` (step conditionné par `hashFiles('scripts/install.sh') != ''`) | contrat de l'installateur POSIX (build release `--locked`, manifeste, binaire exécutable) | **PASS** |
| `miri` | `MIRIFLAGS=-Zmiri-disable-isolation RAVUM_FUZZ_ITERS=16 cargo +nightly miri test -p ravum-lang-syntax -p ravum-lang-sema --lib --tests` | contrôle dynamique Miri sur les crates du langage | **PASS** |
| `fuzz` | `cargo +nightly test -p ravum-lang-syntax --test fuzz_lex_parse -- --ignored --test-threads=1` (garde souple sur `tests/fuzz_*.rs`) | budget long déterministe du fuzz lexer/parser | **PASS** — garde-fou souple, pas une campagne de fuzzing (§7) |
| `wasm` | `cargo build --target wasm32-wasip1 examples/hello` puis `run --target wasm32-wasip1 examples/hello`, stdout `hello, ravum` | build et exécution WASI preview 1 via Node | **PASS** |
| `tsan` | *(désactivé : `if: false`)* | **NOT_APPLICABLE** | **SKIP explicite** — `NOT_APPLICABLE (no threads/async in Core 1.0)` |

### TSan — NOT_APPLICABLE (justifié)

ThreadSanitizer qualifie du code concurrent. Ravum Core 1.0 est **mono-thread**
et n'expose ni threads ni runtime asynchrone ; la FFI est **synchrone** et
mono-thread, et un bloc `unsafe` est transparent pour l'ownership (il n'ouvre
aucune capacité concurrente). Aucun test du corpus ne crée de chemin concurrent
sur un état mutable partagé. Exécuter TSan ici produirait un signal vert sans
rapport avec le produit. Le job est conservé sous forme désactivée
(`if: false`) avec ce commentaire, pour qu'il ne soit pas confondu avec un oubli.
Il devra être réactivé quand un chemin concurrent réel (daemon multi-session,
overlays) sera qualifié avec ses propres tests et sa TCB.

## 5. `scripts/lang-smoke.sh` — port du smoke Windows

Port fidèle de `scripts/lang-smoke.ps1` :

- **31 assertions enregistrées** (comptage des appels `invoke_case` /
  `run_example` et du bloc `init-files` dans le script) : `version`, `doctor`,
  `init`, `init-files`, `init-check`, `init-build`, `init-run` ; pour chacun des
  7 exemples (`hello`, `fib`, `calculator` 5 + 3, `stats` sur `sample.txt`,
  `dsp-gain-iir`, `package-demo/app`, `ffi-abs`) : `check`, `build`, `run`
  (21 assertions) ; `test:tests-demo`, `fmt-check:hello`, et l'échec attendu
  `check-fail-json` → `E-TYP-MISMATCH` (exit 1) ;
- **43 assertions au total pour le job CI `smoke`**, qui ajoute les balayages
  `Stdlib codegen sweep`, `CLI surface sweep`, `CLI documented-flag audit` et
  `JSON contract` (chiffre rapporté par le Lead ; l'API GitHub n'expose que les
  noms d'étapes, pas les compteurs d'assertions) ;
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

- Les valeurs de ce tableau sont l'**instantané local 2026-11-09/10**, conservé
  comme trace de la préparation ; elles ne sont pas rejouées dans la passe du
  2026-11-11. Le smoke enregistre désormais **31 assertions** (§5), pas 28 : la
  ligne « 28 PASS » est historique.
- PyYAML interprète `on:` comme un booléen en mode YAML 1.1 ; la validation
  ci-dessus utilise `BaseLoader` pour garder la clé `on` et vérifier les
  déclencheurs. GitHub Actions utilise sa propre lecture YAML, pas PyYAML.
- Le smoke a été exécuté sous **Git Bash/Windows**, pas sous Linux : il prouve la
  fidélité du port. Le run `smoke` est désormais **PASS** sur `ubuntu-latest`
  (§11) ; c'est lui la référence, pas la ligne locale.
- Le test wasm local utilise Node 26 sur Windows ; la CI utilise une version
  épinglée par `setup-node`. Le chemin CLI est le même, mais la version de Node
  et le système diffèrent : le run CI reste la référence.
- L'installateur `scripts/install.sh` est POSIX et construit `target/release/
  ravum-lang` (sans `.exe`) : il n'a pas été exécuté sous Git Bash/Windows. Sa
  qualification est le job `install` sur Ubuntu, vert au §11.

## 7. Limites et risques résiduels

1. **CI réelle, mais une seule image et un SHA à la fois.** Le run `43026622427`
   est vert sur `ubuntu-latest` à `c67f17e` ; il ne prouve rien pour une autre
   image, un autre noyau ou un autre commit. Les deux échecs de la même journée
   (§11.1) montrent le risque symétrique : **Linux révèle des hypothèses
   Windows** dans nos propres tests. La parade est la règle déjà appliquée —
   assertions explicites par plateforme (`cfg(windows)` / `cfg(unix)`), jamais
   d'affaiblissement — et un rejeu à chaque SHA.
2. **Miri.** `-Zmiri-disable-isolation` autorise les accès fichiers, mais Miri
   n'exécute pas les sous-processus. Si un test de `ravum-lang-sema` ou
   `ravum-lang-syntax` lance `rustc`/un binaire, Miri échouera : le job sera
   rouge et devra être scopé explicitement (`--lib` ou `--skip`) avec une
   justification, pas masqué. `RAVUM_FUZZ_ITERS=16` borne seulement le test fuzz
   par défaut pour éviter un timeout Miri ; le test long reste hors Miri.
3. **Fuzz.** C'est un **garde-fou souple** (tests `fuzz_*` `#[ignore]`), pas une
   campagne de fuzzing ni une couverture exhaustive : il n'établit que l'absence
   de panique/non-déterminisme observée sur ces entrées.
4. **wasm.** WASI preview 1 via Node.js n'est ni du navigateur, ni du GPU, ni du
   component model. `run --target` dépend de Node ≥ 22 et du flag expérimental
   `--experimental-wasi-unstable-preview1`. Si la CLI retire `--target`, le job
   enregistre `NOT_RUN` au lieu d'un faux PASS.
5. **Install.** Le job ne s'exécute que si `scripts/install.sh` est présent. Il
   prouve l'installation sous un préfixe temporaire et `--version` ; il ne prouve
   ni signature, ni provenance, ni SBOM, ni mise à jour.
6. **Conformance.** Depuis cette passe, le job mesure la règle `check ⇒ build`
   (`--verify-build`) et **échoue** si le rapport ne porte pas
   `build_verified: true` ([`CONFORMANCE-BUILD-CONTRACT.md`](CONFORMANCE-BUILD-CONTRACT.md)).
   Le harnais sort 3 si un cas est `NOT_RUN` (environnement indisponible) ; ce
   n'est pas un `PASS`. Le rapport JSON est uploadé même en échec pour l'audit.
   La règle porte sur les modes acceptés ; les `compile-fail` restent des refus
   attendus au `check`.
7. **TSan** n'est pas exécuté (voir §4). Aucune couverture concurrente n'est
   revendiquée.
8. **Caches et image.** Les durées et les versions d'outils dépendent de
   `ubuntu-latest` au moment du run ; le workflow épingle Rust 1.95.0 et Node,
   pas l'image. Aucune mesure de performance n'est produite.
9. **État Git.** Le workflow se déclenche sur `push` ; il ne se lance pas depuis
   la session de l'agent. Les runs de cette passe ont été déclenchés par le
   push du mainteneur.

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

## 9. Suivi — fait, et ce qui reste

**Fait par la passe du 2026-11-11 :**

1. les `UNKNOWN` du §4 sont remplacés par les statuts observés du run
   `43026622427` (URL et SHA cités) ; aucun `PASS` n'est écrit sans run vert ;
2. le job `conformance` mesure désormais la règle `check ⇒ build` et échoue si
   `build_verified: true` manque — ce n'est plus un simple « corpus exécuté » ;
3. les échecs Linux de la journée ont été isolés en **défauts de nos tests**, pas
   du langage, et corrigés sans supprimer ni affaiblir une assertion (§11.1) ;
4. les résultats bruts, durées et artefacts sont consignés au §11.

**Ce qui reste, et qui n'est pas un `TODO` cosmétique :**

- **Rejouer à chaque SHA** : ce document n'est pas transférable. Un run vert sur
  `c67f17e` ne qualifie pas `c6b4537` ni un futur commit ; le job Linux doit
  rester déclenché sur `push` et le résultat reporté ici.
- **Ne pas élargir la revendication** : une seule image (`ubuntu-latest`) et un
  run par SHA ne font pas un support Linux « universel » ; ARM64, macOS et
  d'autres images restent `NOT_RUN` ([`PLATFORMS-MATRIX.md`](PLATFORMS-MATRIX.md)).
- **Côté Windows** : si Windows doit être annoncé autrement que « produit
  testé », il faudra des jobs CI Windows pour la conformance, l'installateur, la
  repro et le SBOM ; ils n'existent pas aujourd'hui (§3.2 de la matrice).
- **Aucun seuil n'est modifié** : la qualification rapporte, elle n'abaisse pas
  les contrats pour obtenir du vert.

## 10. Résultats observés — historique (premier run vert, 2026-11-10)

> Cette section est **conservée telle quelle** : elle documente le premier run
> vert et les correctifs de portabilité de la passe précédente. La référence
> courante est le §11 (run `43026622427`, SHA `c67f17e`, 2026-11-11).

Run final vert : [42675213985](https://github.com/Rwanbt/Ravum/actions/runs/42675213985),
SHA `78c8f72`, `ubuntu-latest`, rustc stable 1.95.0, Node 22, nightly 1.97.0
pour Miri/fuzz.

| Job | Résultat observé | Preuve |
|---|---|---|
| `workspace-tests` | **PASS** | `cargo test --workspace --all-targets -- --test-threads=2`, 8 min environ ; tous les crates/targets compilent et passent sur Linux |
| `conformance` | **PASS** | 107 cas : **105 PASS / 0 FAIL / 2 SKIP** (`security/dependency-absolute-windows`, `security/reserved-windows-name`, spécifiques à Windows) ; artefact `conformance-report-linux` |
| `smoke` | **PASS** | `RESUME: 31 PASS / 0 FAIL / 0 NOT_RUN` (les 31 cas, dont `ffi-abs`) |
| `install` | **PASS** | build release `--locked`, manifeste + SHA-256, binaire `ravum-lang 0.1.0 (edition core-1.0-draft.1)` |
| `fuzz` | **PASS** | `fuzz_lex_parse` long (`--ignored`), budget 200 000 itérations |
| `miri` | **PASS** | libs `syntax` (65) + `sema` (87) + fuzz court (3 itérations), exclusions bornées et documentées |
| `wasm` | **PASS** | `build` + `run --target wasm32-wasip1 examples/hello` → `hello, ravum` via Node 22 |
| `tsan` | **SKIP explicite** | `NOT_APPLICABLE` : pas de threads/async dans Core 1.0, FFI synchrone mono-thread |

### 10.1 Défauts de portabilité Linux trouvés et corrigés

Aucun n'était un défaut du langage ; tous étaient des hypothèses Windows dans
des tests/utilitaires existants. Aucun test n'a été supprimé ou affaibli :

| Commit | Défaut | Correctif |
|---|---|---|
| `92fcc71` | `conformance/util` acceptait `C:\...` comme chemin relatif sous Linux | politique portable : rejet de `\\` et `:` sur tout hôte |
| `d43fffc` | test de rétention du coordinateur cherchait `.dll` en dur | utilise `std::env::consts::DLL_EXTENSION` |
| `8751b43` | `daemon::safe_join` acceptait `C:/abs.rs` sous Linux | rejet des formes absolues Windows et des backslashes |
| `3eb6bb5` | test de supersession dépendant d'un `sleep(40 ms)` (course sur runner rapide) | synchronisation par canal sur l'entrée dans `analyze` |
| `7f50c7e` | stress DSP dépassait 60 s sous contention CI | budget `RAVUM_STRESS_MAX_SECONDS` explicite (défaut local 60 s, CI 1200 s), assertions de correction inchangées |
| `6df3b9b` | `patch-host` tentait de `dlopen` le binaire de test Linux (PIE) | `#[cfg_attr(not(windows), ignore = ...)]` explicite, chemin d'unload couvert sur Windows et par les fixtures cdylib |
| `55cae5a` | cache d'artefact : identifiants ELF reproductibles et borne de temps trop stricte | détection de fraude conservée (`cached_build == false`), borne de temps transformée en sanity bound |
| `008f01a` | test « MSVC non reproductible » échouait sur ELF (reproductible) | revendication limitée à Windows via `#[cfg_attr(not(windows), ignore)]` |

### 10.2 WSL local

`wsl -l -v`, `wsl --shutdown` et toute commande de distribution se bloquent
> 2 minutes ; `Restart-Service WslService` est refusé (pas d'admin). WSL n'a donc
fourni **aucune** preuve. La qualification Linux repose uniquement sur le runner
GitHub, qui est une machine indépendante — un choix de preuve plus fort, mais
qui ne couvre que l'image `ubuntu-latest` du moment.

### 10.3 Incidents de run

Un run intermédiaire ([42673560942](https://github.com/Rwanbt/Ravum/actions/runs/42673560942))
n'a créé que 5 jobs sur 8 (les jobs `install`, `miri`, `fuzz` n'ont pas été
planifiés) tout en rapportant un échec global sans job en échec. Le workflow
était identique dans le commit (vérifié via l'API GitHub). Un commit de relance
a produit le run vert final avec les 8 jobs. Ce run intermédiaire n'est **pas**
compté comme une preuve ; l'incident est probablement côté planificateur GitHub,
mais n'est pas expliqué avec certitude.

## 11. Passe du 2026-11-11 — run `43026622427` (SHA `c67f17e`)

La branche a été poussée à la demande du mainteneur ; la CI s'exécute désormais
réellement sur `push`. Le run de qualification Linux
[43026622427](https://github.com/Rwanbt/Ravum/actions/runs/43026622427) porte sur
le SHA complet `c67f17e033b9bed44694bba0f1dea3ab1b1955cb`. Ses 8 jobs ont été
interrogés via l'API GitHub le 2026-11-11
(`GET /actions/runs/43026622427/jobs?per_page=20` → `total_count = 8`).

### 11.1 Jobs observés

| Job (nom API) | Étapes observées | Conclusion |
|---|---|---|
| Workspace tests (Linux, stable 1.95.0) | `Test workspace (all targets, 2 test threads)` — 13:10:06 → 13:25:14 UTC, ≈ 15 min 08 s | **PASS** |
| Conformance corpus (Linux) | `Run conformance corpus (check ⇒ build contract)` puis `Assert build_verified in the report` | **PASS** |
| Example smoke suite (Linux) | `Run example smoke suite (bash)`, `Stdlib codegen sweep`, `CLI surface sweep`, `CLI documented-flag audit`, `JSON contract (diagnostics.md §7-§8)` | **PASS** (43 assertions, §11.1.1) |
| Fuzz smoke (ignored `fuzz_*` tests, soft guard) | `Run ignored fuzz_* tests when present` | **PASS** — garde-fou souple, pas une campagne (§7) |
| Miri (nightly, syntax + sema) | `Run Miri on the language libraries`, `Run Miri on the short sema fuzz test` | **PASS** |
| wasm32-wasip1 (build + run via Node WASI) | `Build ravum-lang (host)`, `Build and run hello for wasm32-wasip1` | **PASS** |
| Installer contract (temporary prefix, then `--version`) | `Install to a temporary prefix` (le step « Record installer absence honestly » est `skipped`, le script étant présent) | **PASS** |
| ThreadSanitizer — NOT_APPLICABLE | *(aucune étape : job désactivé)* | **SKIP explicite** |

#### 11.1.1 Chiffres non recopiés, et pourquoi

- **Nombre de tests Linux du job `workspace-tests`** : `NOT_MEASURED` dans ce
  document — les logs d'exécution ne sont pas téléchargés (accès authentifié).
  Pour la même révision code, le Lead rapporte **1530 tests côté Windows** ;
  ce chiffre n'est pas une mesure Linux et n'est pas présenté comme telle.
- **Nombre de cas de conformance et de cas vérifiés par `build`** : le rapport
  JSON `conformance-report-linux` est uploadé par le job, mais n'a pas été
  téléchargé ici. La porte est verte **et** l'assertion `build_verified: true`
  est passée ; les compteurs détaillés restent dans l'artefact du run,
  `NOT_MEASURED` localement.
- **43 assertions du job `smoke`** : chiffre rapporté par le Lead ; l'API
  n'expose que les noms d'étapes, pas les compteurs. Le script
  `scripts/lang-smoke.sh` enregistre à lui seul 31 assertions (§5).

### 11.2 Les deux échecs précédents — deux défauts de nos propres tests

Aucun des deux n'était un défaut du **langage**, du CLI ou de la sémantique. Les
deux venaient d'assertions écrites et validées sur Windows uniquement, que
Linux a démasquées. Aucun test n'a été supprimé ni affaibli ; les contrats
vérifiés sont identiques, les assertions sont devenues **explicites par
plateforme**.

| Run | SHA | Conclusion | Défaut de nos tests | Correctif |
|---|---|---|---|---|
| [43020732371](https://github.com/Rwanbt/Ravum/actions/runs/43020732371) | `9da9b12` | `failure` | Deux tests P0-D écrits depuis l'observation Windows seule : ils attendaient que le CLI nomme `300` après `process::exit(300)` et rende `1` pour `exit(256)`. POSIX tronque à 8 bits **avant** que le parent observe : le CLI voit `44` et `0` et n'explique rien. Un second test, `a_killed_build_leaves_the_previous_artifact_running`, courait contre le build (sommeil 120 ms, `try_wait`, `kill`) : si le build se termine entre la sonde et le signal, une publication complète est le résultat correct. | `10459b8` — contrat d'exit code scindé `cfg(windows)` / `cfg(unix)`, assertions sur le libellé de la note, limitation POSIX documentée (`README.md`, `child_exit_code`). `f10a12f` — attendre le début observable de la compilation et n'accepter que deux états cohérents (ancien artefact intact, ou nouveau programme publié avec manifeste cohérent) ; l'invariant protégé (publication atomique) est inchangé. |
| [43023874117](https://github.com/Rwanbt/Ravum/actions/runs/43023874117) | `e84e070` | `failure` | Reste du même défaut d'assertion, révélé par le premier correctif : le test cherchait l'absence de la sous-chaîne `truncat` alors que le répertoire temporaire s'appelait `truncate-*` et que `run` affiche toujours `running: <path>` — le chemin du CLI contenait donc la sous-chaîne. Aucun défaut du CLI. | `c67f17e` — assertions sur le libellé de la note (`exited with status` / `POSIX truncates`), répertoires temporaires renommés `big` et `wraps`, moitiés POSIX type-checkées localement pour `x86_64-unknown-linux-gnu` ; comportement testé inchangé. |
| [43026622427](https://github.com/Rwanbt/Ravum/actions/runs/43026622427) | `c67f17e` | `success` | — | — |

**Ce que ces deux échecs enseignent :** la CI Linux n'est pas décorative — elle a
trouvé deux hypothèses Windows dans nos propres tests, dont une course et une
assertion qui matchait le chemin du binaire. La règle qui en sort : toute
assertion dépendante de l'OS est explicite (`cfg(windows)` / `cfg(unix)`) et le
comportement est vérifié des deux côtés, jamais neutralisé pour obtenir du vert.
Le correctif `10459b8` documente au passage une **limite de plateforme** réelle :
sur POSIX, un programme qui sort avec `256` est indistinguable d'un succès pour
tout parent — ce n'est pas un défaut Ravum et cela ne doit pas être présenté
comme tel.

### 11.3 Le reste du SHA : 12 runs de workflow, tous verts

L'API GitHub associe **12 runs de workflow** au SHA `c67f17e`
(`GET /actions/runs?head_sha=c67f17e…` → `total_count: 12`). Le Lead les rapporte
**tous verts (12/12)** ; le run de qualification Linux `43026622427` est, lui,
vérifié job par job ci-dessus. Les autres runs couvrent les contrats de gate, la
couverture, la documentation, le VS Code E2E et le produit MVP (dont le job
`windows-latest`). Un run vert de ces workflows ne remplace pas la preuve Linux
job par job : chaque affirmation garde son run et son SHA.

### 11.4 Ce que ce run ne prouve pas

- **Une seule image, un seul SHA** : `ubuntu-latest` à `c67f17e`. Rien n'est
  transféré à `c6b4537` (correctif codegen postérieur) ni à une autre image.
- **Pas de performance** : la durée du job (`≈ 15 min 08 s`) est une observation
  sous runner partagé, pas un budget.
- **Pas de TSan** (voir §4) ; pas de threads/async dans Core 1.0.
- **Pas de fuzzing exhaustif** : le job `fuzz` est un garde-fou souple.
- **Pas de WASI « qualifié »** : seul `examples/hello` est construit et exécuté ;
  le corpus n'est pas rejoué sous WASI.
- **Pas de distribution** : l'installateur est testé sous préfixe temporaire,
  sans signature, sans SBOM, sans provenance vérifiable.
- Les points 2–9 du §7 restent valides ; ce run ne les lève pas.

## 12. Rejeu du 2026-11-14 — run `43562818570` (SHA `878e212`)

Le workflow ne se déclenche pas sur `goal/production-ready` (triggers `main`,
`dev`, `feat/**`, `fix/**`) ; il a été **dispatché manuellement** :

```
gh workflow run linux-qual.yml --ref goal/production-ready
```

Objectif : qualifier Linux et **Miri** sur le code des phases 4-5 (`OwnFacts`,
`traps.rs`, garde d'ownership étendu aux places), qui n'existait pas au run de
référence `c67f17e`.

| Job | Résultat exact |
|---|---|
| Workspace Linux | **100 groupes, 1 601 passés / 0 échec / 7 ignorés** |
| Conformance | **476 cas : 474 PASS / 0 FAIL / 2 SKIP**, `build_verified: true` |
| Smoke | **31 PASS / 0 FAIL / 0 NOT_RUN** ; stdlib **64/64** ; CLI **13/13** ; flags **20/20** |
| Miri | syntax **65 passés**, sema **134 passés** (dont 13 de `traps.rs`), fuzz court **2 tests** — 0 échec |
| Fuzz smoke | succès |
| Installer contract | succès (préfixe temporaire, `--version`) |
| wasm32-wasip1 | succès (build + exécution via Node WASI) |
| TSan | `skipped` — `NOT_APPLICABLE` |

Différences avec Windows, assumées et nommées :

- Linux **ignore 7 tests** (contre 5 sous Windows, tests spécifiques au système
  de fichiers) et saute **2 cas de conformance** au lieu d'un :
  `security/dependency-absolute-windows` et `security/reserved-windows-name`,
  tous deux Windows-only. Un SKIP n'est jamais un PASS.
- Miri reste un **contrôle dynamique borné** aux bibliothèques ; il ne remplace
  ni l'audit externe ni une preuve, et Miri local reste `NOT_RUN` (composant
  absent, R5).

Limites : une seule image (`ubuntu-latest`), un seul SHA ; pas de performance,
pas de TSan, pas de WASI qualifié, pas de distribution signée. Ce run ne lève
aucun des points 2–9 du §7.
