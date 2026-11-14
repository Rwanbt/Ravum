# Ravum Core 1.0 — guide de démarrage

> **Statut : DRAFT · `PENDING_HUMAN_APPROVAL` · 2026-11-14.**
> Ce guide décrit le **sous-ensemble Core 1.0 DRAFT** du langage Ravum tel qu'il
> existe sur la branche `goal/consolidation-2` @ **`214bf5e`**, avec le binaire
> `ravum-lang 0.1.0 (edition core-1.0-draft.1)` et `rustc 1.95.0`.
> `214bf5e` est le HEAD au moment des mesures ; il peut avoir avancé depuis, et
> ce guide ne décrit alors pas les commits postérieurs.
>
> **Les commandes de ce guide ont été exécutées le 2026-11-14** sur Windows 11
> x86-64, sous la forme indiquée ou — pour les exemples — sur des copies (voir
> §4), sauf mention explicite `NOT_RUN`. Les **commandes génériques**
> d'installation (`scripts/install.ps1` sans argument, `scripts/install.sh`) et
> `cargo build -p ravum-lang-cli` n'ont **pas** été lancées telles quelles : les
> variantes réellement exécutées et leurs sorties sont citées ci-dessous. Les
> sorties sont réelles et tronquées quand elles sont longues ; les mesures ne
> valent que pour cette machine et ce commit.
>
> **Aucune release n'existe** : aucun tag, aucun artefact signé, aucune
> publication. Ce guide n'est **ni** une déclaration de disponibilité **ni** une
> revendication de maturité : « production ready » est un **objectif décidé**
> (D2), pas un état. L'état des gates est tenu par
> [`../roadmap/CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md) §2 ;
> ce guide ne le remplace pas et ne referme **pas** R14
> ([`../roadmap/PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md)),
> qui reste `PARTIAL` tant que l'audit de véracité n'est pas rejoué par le
> responsable.

## 1. Ce que ce guide couvre — et ce qu'il ne couvre pas

| Couvert (vérifié le 2026-11-14) | Non couvert |
|---|---|
| Installation Windows par `scripts/install.ps1` (installation, `--version`, désinstallation idempotente) | Installation Linux/macOS **locale** : non exécutée ici (`NOT_RUN`) ; Linux est exercé en CI ([`../qualification/PLATFORMS-MATRIX.md`](../qualification/PLATFORMS-MATRIX.md) §2) |
| Paquet minimal, `check`/`build`/`run`/`test`/`fmt`/`lint`/`doc`/`cache`/`doctor`/`lsp` | `watch` en fonctionnement continu, `--filter`, profils `pure`, débogueur, couverture |
| Sortie texte, sortie JSON `schema_version: 1`, timings | Mesures de performance de référence (voir [`../reports/LANG-TIMINGS.md`](../reports/LANG-TIMINGS.md)) |
| Les 10 exemples de [`examples/`](../../examples/) exécutés | LSP à l'échelle en profil release : **mesuré** (p95 163,5 ms à 5 110 lignes), cible 50 ms non tenue |
| Refus explicites (threads, `async`, traits, FFI non scalaire) | Audit externe, signature, Go/No-Go humain |

Ce guide décrit **un sous-ensemble**. La surface exacte est
[`../../spec/language/unsupported.md`](../../spec/language/unsupported.md)
(liste de fermeture des refus, avec code de diagnostic) et
[`../../spec/language/features.md`](../../spec/language/features.md) (règles
tracées). La spécification est **DRAFT**, non ratifiée ; les ADR restent
`PROPOSED`.

## 2. Installer

### 2.1 Prérequis

| Élément | Valeur constatée |
|---|---|
| Shell Windows | PowerShell 7+ (`pwsh`), `#requires -Version 7.0` dans `scripts/install.ps1` |
| Rust | toolchain **1.95.0**, épinglée par [`rust-toolchain.toml`](../../rust-toolchain.toml) (`channel = "1.95.0"`, composant `rustfmt`) |
| Cargo | dans le `PATH` |
| Réseau | non requis par les scripts : build `--locked` sur le cache local ([`../release/INSTALL.md`](../release/INSTALL.md) §1) |
| Droits | aucun administrateur : installation dans le profil utilisateur |

### 2.2 Windows — `scripts/install.ps1`

```powershell
# Depuis la racine du dépôt
pwsh -File scripts/install.ps1
```

| Option | Effet (vérifié dans l'entête du script et dans [`INSTALL.md`](../release/INSTALL.md) §2) |
|---|---|
| `-Prefix <DIR>` | préfixe d'installation (défaut `%LOCALAPPDATA%\Ravum`) |
| `-AddToPath` | ajoute `<Prefix>\bin` au `PATH` **utilisateur** (idempotent ; nouveau terminal requis) |
| `-SkipBuild` | réutilise `target\release\ravum-lang.exe` déjà construit |
| `-Uninstall` | supprime binaire + manifeste + `bin` vide (idempotent) |
| `-RemoveFromPath` | avec `-Uninstall`, retire `<Prefix>\bin` du `PATH` utilisateur |

Le script écrit `<Prefix>\bin\ravum-lang.exe` et `<Prefix>\ravum-install.json`,
ne modifie **aucun** fichier du dépôt hors `target/`, et **ne signe rien** (aucune
clé n'existe : D9 différée).

**Exécution réelle du 2026-11-14** (préfixe temporaire, `-SkipBuild` pour ne pas
relancer un build lourd — les autres agents compilaient) :

```text
pwsh -File scripts/install.ps1 -Prefix target\guide-verify\install -SkipBuild
ravum-install: installe : ravum-lang 0.1.0 (edition core-1.0-draft.1)
ravum-install: sha256   : 25987c3bac881557dcfdae1cfd8571ead8dadfaa19619de470b549ea0e8f8879
ravum-install: commit   : 214bf5e7b1fb60082b684266aa9d0b099fb823aa
ravum-install: PATH non modifie. Ajouter : ...\install\bin (ou relancer avec -AddToPath).
ravum-install: warning: arbre de travail modifie (git_dirty=true) : le binaire peut
               contenir des changements non commités.
exit 0
```

Le manifeste produit (extrait réel) porte `"locked": true`, `"platform":
"windows"`, `"arch": "AMD64"`, `"rustc": "rustc 1.95.0 (59807616e 2026-04-14)"`,
`"sha256"` recalculé après copie, et `"installer": "scripts/install.ps1"`.
`git_dirty: true` signifie que le binaire **ne correspond pas exactement** au
commit enregistré : un tel binaire n'est pas un artefact de release
([`../release/PROVENANCE.md`](../release/PROVENANCE.md)).

Désinstallation vérifiée deux fois de suite :

```text
pwsh -File scripts/install.ps1 -Prefix target\guide-verify\install -Uninstall
ravum-install: desinstalle : ...\bin\ravum-lang.exe, ...\ravum-install.json, ...\bin
exit 0

pwsh -File scripts/install.ps1 -Prefix target\guide-verify\install -Uninstall
ravum-install: rien a desinstaller (prefixe : ...\install)
exit 0
```

Vérification du binaire installé :

```powershell
& "$env:LOCALAPPDATA\Ravum\bin\ravum-lang.exe" --version
# ravum-lang 0.1.0 (edition core-1.0-draft.1)
```

### 2.3 Linux — `scripts/install.sh`

```sh
# Depuis la racine du dépôt
bash scripts/install.sh
```

Options : `--prefix DIR` (défaut `$RAVUM_PREFIX` ou
`$HOME/.local/share/ravum`), `--add-to-path` (affiche la ligne `export PATH=...`,
n'écrit **aucun** fichier de profil), `--skip-build`, `--uninstall`,
`--remove-from-path` ([`INSTALL.md`](../release/INSTALL.md) §3). Sous Git
Bash/MSYS, un chemin Windows est converti par `cygpath` ; sans `cygpath` le
script refuse le chemin (code 2).

Le contrat public de l'installateur Linux est
[`docs/release/INSTALL-LINUX.md`](../release/INSTALL-LINUX.md) (**`PROPOSED`**) :
il fige le préfixe, le manifeste, les codes de sortie et la désinstallation
idempotente, et il est exercé par `scripts/install-contract.sh` dans
`linux-qual.yml`. La référence de base reste
[`docs/release/INSTALL.md`](../release/INSTALL.md) §3 et
[`docs/qualification/PLATFORMS-MATRIX.md`](../qualification/PLATFORMS-MATRIX.md)
§2.

**État de qualification Linux** : `PASS (CI)` — `scripts/install.sh` s'exécute
dans un préfixe temporaire avec `--version` dans le job `install` de
`linux-qual.yml` (matrice des plateformes, §2 et [`INSTALL.md`](../release/INSTALL.md)
§3). Le contrat élargi (manifeste complet, SHA-256 installé = construit,
désinstallation + réinstallation, codes d'usage) est ajouté à ce même job ; son
exécution est `PENDING` jusqu'au prochain run vert et ne doit pas être annoncée
comme acquise. **Aucune exécution locale Linux dans cette passe (`NOT_RUN`)** :
macOS n'a **jamais** été exécuté, et la présence de branches `darwin` dans le
script n'est pas une preuve de support.

### 2.4 Sans installer

Les deux binaires existent déjà dans le dépôt après un build :

```powershell
target\debug\ravum-lang.exe --version     # profil dev
target\release\ravum-lang.exe --version   # profil release
# ravum-lang 0.1.0 (edition core-1.0-draft.1)
```

Pour reconstruire (non exécuté dans cette passe, cf. contrainte de builds
sérialisés) :

```powershell
cargo build -p ravum-lang-cli
```

Diagnostic d'environnement :

```powershell
target\debug\ravum-lang.exe doctor --json
```

Sortie réelle (extrait) : `"verdict": "ready"`, `rustc` = `rustc 1.95.0`, stades
`parser`, `checker`, `codegen` = `ready`, `cache` = `absent`. `doctor` signale un
`rustc` absent ou un environnement non supporté ; cela n'indique pas que
l'installation du binaire a échoué.

## 3. Premier paquet

```powershell
target\debug\ravum-lang.exe init demo
```

Sortie réelle :

```text
created package `demo` in demo
  demo\ravum.toml
  demo\src/main.rv
  demo\.gitignore
next: cd demo && ravum-lang check
```

`ravum.toml` minimal produit (contenu exact) :

```toml
[package]
name = "demo"
version = "0.1.0"
edition = "core-1.0"
```

`src/main.rv` produit (contenu exact) :

```rv
// Ravum Core 1.0 — generated by `ravum-lang init`.
use std::io;
fn main() {
    io::println("hello, ravum");
}
```

Ce paquet est **bin** : une racine `src/main.rv`. Un paquet bibliothèque a une
racine `src/lib.rv` (`init --lib`) et il **ne peut pas** avoir les deux racines
dans le même paquet (`E-MOD-BOTH-ROOTS`, [`unsupported.md`](../../spec/language/unsupported.md)
§6). Une dépendance locale s'écrit avec un chemin relatif :

```toml
[dependencies]
util = { path = "../util" }
```

(forme réelle de [`examples/package-demo/app/ravum.toml`](../../examples/package-demo/app/ravum.toml)).
Aucun registre, aucune plage de versions, aucune dépendance distante
(`E-MOD-DEPENDENCY`).

Règles de résolution du `PATH` (entête de `ravum-lang --help`) : un répertoire
doit contenir `ravum.toml` ; un fichier `.rv` seul est chargé comme programme
autonome, sans manifeste ni lockfile. Vérifié : `check examples\hello\src\main.rv`
→ `package main (file, ...)`, `lockfile: not_applicable`, exit 0.

## 4. La boucle `check` → `build` → `run` → `test`

> Les exemples du dépôt contiennent déjà un cache `.ravum/` (ignoré par Git).
> Pour ne pas modifier le dépôt, les commandes ci-dessous ont été exécutées sur
> des **copies** sous `target/guide-verify/` ; les sorties citées sont celles de
> ces copies. Sur une copie fraîche de `examples/hello`, les chemins affichés
> diffèrent mais le résultat est identique.

### 4.1 `check` — analyse

```powershell
target\debug\ravum-lang.exe check examples\hello
```

```text
package hello (bin, profile core): 1 file(s), 2 item(s), 1 function(s), 0 test(s), 0 error(s), 0 warning(s)
lockfile: current
exit 0
```

La première analyse d'un paquet **écrit** `ravum.lock` (sortie observée :
`lockfile: created`). `--locked` exige le lockfile et refuse son absence —
vérifié sur un paquet `init` non encore analysé :

```text
target\debug\ravum-lang.exe check demo --locked
error[E-MOD-MISSING]: `--locked` was requested but `...\demo\ravum.lock` does not exist;
      run `ravum-lang check` without `--locked` to create it
exit 1
```

Options utiles (entête `--help`, exécutées) :

| Commande | Effet |
|---|---|
| `check [PATH] --json` | enveloppe JSON versionnée (voir §5) |
| `check [PATH] --timings` | une ligne `timings:` avec `load/parse/resolve/typeck/effects/own/total` |
| `check [PATH] --profile core\|pure\|rt` | profil d'effets |
| `check [PATH] --rt-entry FN` | point d'entrée analysé sous profil `rt` (voir §8, `dsp-gain-iir`) |
| `check [PATH] --locked` | échoue si le lockfile manque ou est périmé |

### 4.2 `build` — génération Rust + `rustc`

```powershell
target\debug\ravum-lang.exe build examples\hello
```

```text
generated: ...\.ravum\gen\main.rs
built: ...\.ravum\bin\hello.exe
identity: sha256:e035bac2f0581b3bef88cd0b838faa2423406f44fcc8634ed975a55fde95ce69
manifest: ...\.ravum\bin\hello.exe.build.json
isolated: ...\.ravum\build\e035bac2f0581b3b
exit 0
```

`build` publie un artefact **atomique** dans `.ravum/bin/<nom>` avec un manifeste,
et l'identité `sha256:` déterministe. `--emit-rust <FILE>` conserve le Rust généré
pour audit ; `--timings` ajoute `codegen` et `rustc` (voir §5). La chaîne est la
voie B : `ravum-lang` génère du Rust sûr, puis appelle `rustc`. Aucun backend
natif direct n'existe (voie C `NOT_IMPLEMENTED`, T1 `PARTIAL`).

### 4.3 `run` — exécution

```powershell
target\debug\ravum-lang.exe run examples\hello
# hello, ravum

target\debug\ravum-lang.exe run examples\calculator -- 5 + 3
# 8

target\debug\ravum-lang.exe run examples\fib
# 11 lignes, une valeur par ligne : 0 1 1 2 3 5 8 13 21 34 55
```

Le séparateur `--` transmet les arguments **au programme** (le CLI les sépare,
`--help`). **Le processus enfant est lancé avec la racine du paquet comme
répertoire courant** — vérifié dans le code
(`crates/ravum-lang-cli/src/commands/run.rs`, `command.current_dir(frontend.root())`)
et par le comportement : depuis la racine du dépôt,
`run <pkg> -- sample.txt` réussit, tandis que
`run <pkg> -- <pkg>\sample.txt` échoue (`cannot read file ... os error 3`) parce
que le chemin relatif est résolu depuis le paquet. Conséquence pratique : passer
des chemins relatifs au paquet, ou des chemins absolus.

Le même répertoire courant s'applique à `test`
(`crates/ravum-lang-cli/src/commands/test.rs`).

### 4.4 `test` — tests unitaires

`test fn` déclare un test ; `assert` et `assert_eq` sont utilisés par l'exemple,
et `-> Result<(), Error>` avec `?` est accepté (une fonction `test fn` peut
échouer par `Err`).

```powershell
target\debug\ravum-lang.exe test examples\tests-demo
```

```text
running 3 tests
test addition_works ... ok
test strings_work ... ok
test result_propagation_works ... ok

test result: ok; 3 passed; 0 failed; 0 ignored; 0 filtered out
test cases: 3 declared; 0 filtered out; 3 executed
exit 0
```

`test --json` renvoie `summary.total`, `passed`, `failed`, `ignored`,
`filtered` et la liste `tests[]`. `--filter TEXT` existe (entête `--help`) mais
n'a **pas** été exercé ici (`NOT_RUN`).

### 4.5 `watch` — reconstruction sur modification

`watch` (alias `dev`) reconstruit à chaque changement de source, avec
`--run` pour relancer. **Le rechargement à chaud du langage est un redémarrage :
aucun état n'est préservé** ([`../reports/LANGUAGE-CORE-STATUS.md`](../reports/LANGUAGE-CORE-STATUS.md)
§1.4, [`unsupported.md`](../../spec/language/unsupported.md) §5). Cette commande
n'a **pas** été exercée dans cette passe (`NOT_RUN`) ; voir
`scripts/watch-fail-closed-check.ps1` pour le contrat fail-closed déjà outillé.

### 4.6 Cible `wasm32-wasip1` — `NOT_RUN` en local

```powershell
target\debug\ravum-lang.exe run --target wasm32-wasip1 examples\hello
```

Sur cette machine, la commande **échoue** faute de cible installée :

```text
error: rustc failed:
error[E0463]: can't find crate for `std`
  = note: the `wasm32-wasip1` target may not be installed
  = help: consider downloading the target with `rustup target add wasm32-wasip1`
exit 1
```

Prérequis réel : `rustup target add wasm32-wasip1`, et Node.js (WASI est exécuté
via Node). L'état `PASS` de la matrice des plateformes a été obtenu **avec** ces
prérequis sur la machine de mesure
([`../qualification/PLATFORMS-MATRIX.md`](../qualification/PLATFORMS-MATRIX.md)
§2, cellule WASI) ; il n'est pas reproduit ici.

## 5. Sortie JSON et timings

### 5.1 Enveloppe JSON

`check`, `test` et `doctor` acceptent la forme `--json`, sur **stdout**
uniquement, `schema_version: 1`. L'enveloppe **réellement observée** contient plus
de champs que le résumé de l'entête `--help` :

```json
{"schema_version":1,"tool":"ravum-lang","tool_version":"0.1.0",
 "spec_version":"core-1.0-draft.1","command":"check","status":"success",
 "exit_code":0,"success":true,"target":{"kind":"package","path":"examples/hello"},
 "profile":"core","dependencies_digest":"sha256:e3b0c442...","lockfile_stale":false,
 "summary":{...},"diagnostics":[]}
```

Un consommateur doit lire **par nom de champ** et tolérer des champs
supplémentaires : la forme minimale documentée par `--help` est
`{schema_version, command, success, diagnostics, summary}`.

Chaque diagnostic porte `code`, `severity`, `phase`, `message`, `file`, `line`,
`column`, `end_line`, `end_column`, `primary` (bornes en octets, lignes/colonnes
UTF-16), `labels`, `notes`. Exemple réel (`check` sur un programme fautif) :

```json
{"code":"E-TYP-MISMATCH","severity":"error","phase":"types",
 "message":"let: expected `i32`, found `&text`","file":"src/main.rv",
 "line":3,"column":18,"end_line":3,"end_column":25, ...}
```

> Sur le même programme, le texte humain (sur **stderr**) et le JSON (sur
> **stdout**) portent les mêmes diagnostics. L'ordre de production est spécifié
> comme déterministe ([`diagnostics.md`](../../spec/language/diagnostics.md) §4) ;
> cette passe n'a pas mesuré le déterminisme lui-même.

### 5.2 Codes de sortie

| Code | Sens (entête `--help`) | Reproduit ici |
|---|---|---|
| `0` | succès | oui (`check`, `build`, `run`, `test`, `fmt`, `lint`, `doc`) |
| `1` | échec de compilation/diagnostic/test ou refus opérationnel sûr | oui (`E-TYP-MISMATCH`, `--locked` sans lockfile, `rt` avec `io`) |
| `2` | ligne de commande invalide | `NON REPRODUIT` (documenté par `--help`) |
| `3` | outil externe manquant (`rustc`) | `NON REPRODUIT` (documenté par `--help`) |

### 5.3 Timings

`--timings` ajoute une ligne unique. Valeurs observées **une fois** sur cette
machine (ce ne sont pas des benchmarks) :

```text
# check (binaire debug, examples/hello)
timings: load=0.911ms parse=0.067ms resolve=0.063ms typeck=0.103ms effects=0.036ms own=0.018ms total=1.197ms

# build (binaire debug, copie de examples/hello)
timings: load=1.386ms parse=0.080ms resolve=0.073ms typeck=0.095ms effects=0.062ms own=0.021ms codegen=2.566ms rustc=680.415ms total=684.698ms
```

`build` est dominé par `rustc` sur les petits paquets ; à l'échelle, c'est
l'analyse d'ownership qui domine (voir §10). Les campagnes de référence
sont dans [`../qualification/PERFORMANCE.md`](../qualification/PERFORMANCE.md) et
[`../reports/LANG-TIMINGS.md`](../reports/LANG-TIMINGS.md) — chaque chiffre y est
lié à une machine, un profil et un SHA.

## 6. Formateur

```powershell
target\debug\ravum-lang.exe fmt examples\hello\src\main.rv   # réécrit le fichier
target\debug\ravum-lang.exe fmt examples\hello --check       # ne réécrit rien
```

Sortie réelle du mode `--check` (fichier déjà formaté) :

```text
fmt: 1 file(s) checked, 0 differ
exit 0
```

Et sur un fichier volontairement mal espacé :

```text
would reformat ...\bad.rv
fmt: 1 file(s) checked, 1 differ
exit 1

# après `fmt ...\bad.rv` :
reformatted ...\bad.rv
fmt: 1 file(s) checked, 1 written
exit 0
```

`--check` sort en `1` si un fichier serait modifié. Le formateur est
**idempotent** et préserve commentaires et lignes vides (T3 `PASS`) ; il n'a
**aucune option de configuration** (LF + indentation fixes, « formateur
configurable : partiel » dans [`unsupported.md`](../../spec/language/unsupported.md)
§7). Le serveur LSP expose le même formateur via `textDocument/formatting`.

## 7. Serveur de langage (`lsp`) et harnais VS Code

### 7.1 Démarrer le serveur

```powershell
target\debug\ravum-lang.exe lsp
```

JSON-RPC 2.0 sur **stdio**, base protocol LSP (`Content-Length`), LSP 3.17 ;
seuls des octets de protocole sont écrits sur `stdout`, les erreurs vont sur
`stderr`. Échange réel `initialize` → `shutdown` → `exit` (exit 0) :

```text
{"id":1,"jsonrpc":"2.0","result":{"capabilities":{
  "completionProvider":{"triggerCharacters":[".",":"]},
  "definitionProvider":true,"documentFormattingProvider":true,
  "documentSymbolProvider":true,"hoverProvider":true,"referencesProvider":true,
  "renameProvider":{"prepareProvider":true},
  "textDocumentSync":{"change":2,"openClose":true,"save":{"includeText":true}},
  "workspaceSymbolProvider":true},
  "positionEncoding":"utf-16",
  "serverInfo":{"name":"ravum-lang-lsp","version":"0.1.0"}}}
```

Capacités **implémentées** : cycle de vie, synchronisation incrémentale
(`change: 2`), buffers non sauvegardés analysés depuis la mémoire,
`publishDiagnostics` (mêmes codes que `check`), `hover`, `definition`,
`references`, `rename` + `prepareRename`, `completion`, `documentSymbol`,
`workspace/symbol`, `formatting`. **Non implémentées** — le serveur répond par
une erreur JSON-RPC `-32601`, jamais par un succès simulé : `codeAction`,
`semanticTokens`, `inlayHint`, `signatureHelp`, `prepareCallHierarchy`,
`workspace/applyEdit` ([`../qualification/LSP.md`](../qualification/LSP.md) §1–§2).

Configuration client minimale ([`LSP.md`](../qualification/LSP.md) §6) :

```json
{
  "command": "ravum-lang",
  "args": ["lsp"],
  "filetypes": ["ravum"],
  "rootPatterns": ["ravum.toml"]
}
```

### 7.2 Harnais VS Code

L'extension **de développement** est présente dans le dépôt sous
`tools/vscode-ravum` (`publisher.ravum-lang` : `package.json`,
`client/extension.js`, `syntaxes/ravum.tmLanguage.json`,
`test/runTest.js`). Le harnais a été exécuté le 2026-11-13 en session réelle :
**10 tests, 10 PASS / 0 FAIL** avec VS Code 1.145.2, serveur
`target/debug/ravum-lang.exe lsp`
([`../qualification/VSCODE.md`](../qualification/VSCODE.md)).

```powershell
cargo build -p ravum-lang-cli
cd tools/vscode-ravum
npm install --no-audit --no-fund
node test/runTest.js
```

Variables : `RAVUM_LSP_BIN` (chemin du binaire) et `RAVUM_E2E_VSCODE` (chemin
d'un `Code.exe`).

> **`NOT_RUN` dans cette passe** : je n'ai pas lancé le harnais VS Code (il ouvre
> une fenêtre Electron et exige `npm install`). Le résultat 10/10 est celui du
> document daté, pas une re-mesure.
>
> **Incohérence documentaire à vérifier** : [`LSP.md`](../qualification/LSP.md)
> §6 affirme que « l'extension VS Code n'est pas fournie dans ce dépôt à ce
> stade », alors que `tools/vscode-ravum` existe et que
> [`VSCODE.md`](../qualification/VSCODE.md) documente son exécution. Les deux
> documents sont des documents maîtres : la contradiction est signalée ici,
> **non corrigée** (marqué `À VÉRIFIER`).

## 8. Exemples disponibles

Les **10** paquets de [`examples/`](../../examples/) sont des programmes `.rv`
réellement compilés et exécutés. Tous ont été exécutés le 2026-11-14 (copies sous
`target/guide-verify/`) :

| Exemple | Commande vérifiée | Ce qu'il montre |
|---|---|---|
| `hello` | `run` → `hello, ravum` | premier programme, `io::println`, sortie 0 |
| `fib` | `run` → `0 1 1 2 3 5 8 13 21 34 55` | récursion, `while`, cast `as`, `Result`/`?` |
| `calculator` | `run -- 5 + 3` → `8` | arguments, `f64`, `Result`, `?`, comparaison de texte |
| `stats` | `run -- sample.txt` → `2.5` | `fs::read_to_string`, lignes, `parse_f64`, moyenne |
| `file-tool` | `run -- in.txt out.txt` → `done`, `BONJOUR RAVUM` | Unicode, `fs::write_string`, codes de sortie |
| `file-store` | `run -- store put cle1` → `stored cle1 v1` ; `get` → valeur ; `list` ; `history` | magasin clé/valeur versionné, **pas** une base de données ; répertoire à créer à la main (`create_dir` absent) |
| `package-demo` | `run app` → `42` puis `40` | paquet multi-module + dépendance locale `path = "../util"` |
| `tests-demo` | `test` → `3 passed` | `test fn`, `assert_eq`, test `-> Result<(), Error>` |
| `ffi-abs` | `run` → `42` | `extern "C"` + `unsafe`, **FFI scalaire seulement** (voir §10.2) |
| `dsp-gain-iir` | `run` → `2 1 -0.5 0 --- 0.2 ...` ; `check --profile rt --rt-entry process` → 0 erreur | DSP **hors-ligne** (gain + IIR sur buffers fixes), aucun périphérique audio |

Le profil `rt` du dernier exemple illustre un point réel : la même commande
**sans** `--rt-entry` échoue, car `main` (qui appelle `io::println`) devient le
point d'entrée analysé :

```text
target\debug\ravum-lang.exe check examples\dsp-gain-iir --profile rt
error[E-EFF-PROFILE]: profile `rt` forbids effect `io` reachable from `main`
... 5 erreurs (io + indexations et arithmétique pouvant paniquer)
exit 1
```

Le profil `rt` est **expérimental** : il ne certifie pas le temps réel
([`unsupported.md`](../../spec/language/unsupported.md) §5 ; gate de traps
`RFC-RT-TRAPS-1, PENDING_HUMAN_APPROVAL`).

Le dossier contient aussi des fichiers `.ul` historiques : ce sont des **idées de
syntaxe**, pas du code compilable ([`README.md`](../../README.md), paragraphe
« Les fichiers `.ul` des exemples initiaux »).

## 9. Diagnostics et codes

### 9.1 Où sont les codes

Le registre canonique est [`../../spec/language/diagnostics.md`](../../spec/language/diagnostics.md)
§3 (lexique/syntaxe, résolution, modules, typage, propriété, effets,
avertissements) avec le sens, la phase et la sévérité de chaque code. Le schéma
JSON versionné est au §7 du même document. L'audit outillé
`python tools/registry_audit.py` vérifie que le compilateur n'émet aucun code
hors registre : **108 codes enregistrés / 126 émis / 0 hors registre**, chiffre
de la passe 12 ([`../roadmap/CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md)
§0 octies, 2026-11-13), re-mesuré le 2026-11-14
([`../reports/LANGUAGE-CORE-STATUS.md`](../reports/LANGUAGE-CORE-STATUS.md) §1.1).

R10 (interfaces stables) reste **`PARTIAL`** : le registre est audité, mais la
**politique de dépréciation n'est pas ratifiée** et le gel du registre de codes
ne l'est pas non plus
([`../roadmap/PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) §3 et
R10). Aucun code n'est donc promis comme définitif.

### 9.2 Exemples de diagnostics réels

| Programme | Diagnostic | Où |
|---|---|---|
| `let x: i32 = "texte";` | `E-TYP-MISMATCH` + `W-OWN-UNUSED-VARIABLE` | texte (stderr) et JSON (stdout), exit 1 |
| `let t = thread::spawn(work);` | `E-RES-UNKNOWN` (nom `thread`) + `E-TYP-CALL` (fonction utilisée comme valeur) | exit 1 |
| `async fn f()` | `E-UNSUPPORTED-ASYNC` | exit 1 |
| `trait T { ... }` | `E-UNSUPPORTED-TRAIT` | exit 1 |
| paquet sans lockfile + `--locked` | `E-MOD-MISSING` | exit 1 |
| `io` sous profil `rt` | `E-EFF-PROFILE` | exit 1 |

`lint` rapporte les avertissements et `--deny CODE` transforme l'un d'eux en
échec — vérifié :

```text
target\debug\ravum-lang.exe lint <pkg> --deny W-OWN-UNUSED-VARIABLE
lint: 1 warning(s), 1 error(s), 1 denied warning(s) (deny: W-OWN-UNUSED-VARIABLE)
exit 1
```

Le texte humain est destiné à l'humain, le JSON à l'outil (P0-E). Un agent doit
lire `diagnostics[].code`, pas le message.

## 10. Limites explicites de Core 1.0

### 10.1 Concurrence, réseau, asynchrone

**`UNSUPPORTED`, refusé par le vérificateur, jamais silencieux** :

- **threads** : `thread::spawn`, `JoinHandle`, `AtomicUsize`, `Mutex`, `RwLock`,
  `Arc`, canaux → `E-RES-UNKNOWN` (§9.2) ; `CONC-1.1` : mono-thread ;
- **réseau** : effet `net`, toute API réseau → `E-UNSUPPORTED-EFFECT` ;
- **async/await** : `async fn`, `.await` → `E-UNSUPPORTED-ASYNC` ;
- **macros** (`macro_rules!`, `#[derive]`, attributs), **traits**, **closures**,
  `static`, alias de type, `union`, `where`, génériques bornés → codes dédiés.

Table exhaustive : [`unsupported.md`](../../spec/language/unsupported.md) §2–§5.

### 10.2 FFI : scalaire uniquement, et phrase TCB

Sont admis **exactement** `i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 isize usize
f32 f64 bool ()` derrière `extern "C"`, appelés **dans un bloc `unsafe`** ; seul
l'ABI `"C"` existe ; `char`, `String`, `Vec`, `&T`, `&str`, `&mut T`, tuples,
tableaux, structs/enums sont refusés (`E-FFI-TYPE`) ; `unsafe fn`, `*const`,
`*mut`, `&raw` sont refusés (`E-UNSUPPORTED-UNSAFE`) ; aucun `#[link]`, aucun
`-l`, aucun chemin de bibliothèque : **seuls les symboles déjà liés par défaut**
(CRT/libm) sont atteignables. Ni buffer, ni chaîne, ni struct, ni handle, ni
callback, ni ABI stable
([`../ffi/SAFETY.md`](../ffi/SAFETY.md) §2, §4).

> **Phrase de la base de confiance (TCB), sans adoucissement** : *appeler une
> fonction C place cette bibliothèque dans la base de confiance du processus.*
> Un programme Ravum peut donc être corrompu par du code que Ravum n'a pas
> analysé. La sûreté garantie est une **sûreté de forme** (un seul `unsafe`,
> types autorisés fermés), **pas** une sûreté mémoire de la bibliothèque C
> ([`SAFETY.md`](../ffi/SAFETY.md) §1 et C-FFI-10). La FFI est
> **expérimentale** ; D7 reste **ouvert** (`PENDING_HUMAN_APPROVAL`), et les
> profils `pure` et `rt` la refusent (`E-EFF-PROFILE`).

### 10.3 Pas de verticale DSP `.rv` → hot swap

Il n'existe **aucune** verticale DSP du langage connectée à un hôte audio :
c'est un constat daté (« Verticale DSP `.rv` → hot swap : **non faite, RFC
seule** », [`../audit/2026-11-14-architecture-audit.md`](../audit/2026-11-14-architecture-audit.md)
§ constats ; T9 = « 10 verticales, **pas de DSP hôte `.rv`** »,
[`../reports/GOAL-PRODUCTION-READINESS-REPORT.md`](../reports/GOAL-PRODUCTION-READINESS-REPORT.md)).
`examples/dsp-gain-iir` calcule un gain et un IIR **hors-ligne** sur des buffers
fixes : aucun périphérique, aucun callback temps réel, aucune migration d'état.
Le profil `rt` vérifie des **effets et des traps** à l'entrée déclarée ; il ne
certifie ni latence, ni WCET, ni hard-RT. La plateforme Rust historique
(MVP-0a/M3, hot patch `f64 -> f64`, crossfade) est une **autre ligne**, décrite
dans [`README.md`](../../README.md) ; elle ne compile pas de `.rv`.

### 10.4 LSP à l'échelle : la cible 50 ms n'est pas tenue

Mesures publiées ([`../qualification/LSP.md`](../qualification/LSP.md) §7,
[`../reports/LANG-TIMINGS.md`](../reports/LANG-TIMINGS.md) §9), harnais
`cargo test` (profil de développement, binaire debug, transport JSON-RPC
inclus) :

| Corpus | Lignes | Éditions | p50 | p95 | Cible P0-A « p95 < 50 ms » |
|---|---:|---:|---:|---:|---|
| `package-demo` (2 fichiers) | 338 | 20 | 2,061 ms | **2,826 ms** | tenue |
| corpus généré (150 modules) | **5 110** | 20 | 322,4 ms | **337,6 ms** | **non tenue** |

À 5 110 lignes, le coût est celui du `check` complet (364,9 ms p50), car
l'analyse sémantique est **globale** : le cache de parse ne réduit que le
parsing. La cible reste tenue sur les petits paquets.

> **Mesure en profil release : mesurée le 2026-11-14 sur `214bf5e`.**
> `cargo test --release -p ravum-lang-cli --test lsp_client
> end_to_end_diagnostic_latency_on_the_scale_corpus -- --ignored --nocapture`
> → **p50 116,7 ms / p95 163,5 ms** à 5 110 lignes (20 éditions, transport
> inclus). Le profil release divise la latence debug par ~2, mais **reste
> ×3,3 au-dessus de la cible 50 ms**. Le chiffre debug (337,6 ms) n'est pas un
> chiffre produit ; le chiffre release n'est pas une cible tenue. La décision de
> périmètre ou d'incrémentalité sémantique est humaine
> ([`../reports/HUMAN-DECISIONS-REQUIRED.md`](../reports/HUMAN-DECISIONS-REQUIRED.md),
> `D-2026-11-14-5`).

### 10.5 Performance de `check` : non linéaire à l'échelle

Chiffres publiés ([`../reports/LANG-TIMINGS.md`](../reports/LANG-TIMINGS.md) §8,
corpus synthétique, branche `goal/consolidation-2`) :

| Lignes | `check` p50 | dont `own` | `own` % |
|---:|---:|---:|---:|
| 1 025 | 34,0 ms | 11,5 ms | 33,8 % |
| 5 110 | 364,9 ms | 261,6 ms | 71,7 % |
| 20 110 | 4 625,0 ms | 4 342,6 ms | 93,9 % |
| 50 110 | **26 423,5 ms** | **25 758,3 ms** | **97,5 %** |

Entre 5 110 et 50 110 lignes (×9,8 de code), `check` est multiplié par **72** ;
la cause n'est **pas** établie par un profileur (fait mesuré, pas diagnostic).
Ce n'est pas `rustc` : à 50 110 lignes il ne pèse que 5,6 % du build. Sur les
petits paquets, `check` reste à l'échelle de la milliseconde
([`../qualification/PERFORMANCE.md`](../qualification/PERFORMANCE.md) :
38,7 ms médians à 1 106 lignes, 86,4 ms à 5 506, profil `dev`).

### 10.6 Ce qui n'existe pas du tout

- **aucun tag, aucune release, aucun artefact signé** : `git tag` est **vide**
  au `214bf5e` (vérifié), la checklist de release est **candidate, non
  ratifiée** ([`../release/RELEASE-CHECKLIST.md`](../release/RELEASE-CHECKLIST.md)),
  et D9 (custody de clé) est **différée** — le mécanisme de signature OpenSSH
  est qualifié, mais **aucune clé** n'existe
  ([`../qualification/SIGNATURES.md`](../qualification/SIGNATURES.md)) ;
- **aucun backend natif direct** : voie C `NOT_IMPLEMENTED`, décision M5 non
  ratifiée ;
- **aucun moteur de preuves Ravum**, **aucun prouveur externe installé**, **aucun
  audit externe** ;
- **aucune préservation d'état** au rechargement (redémarrage) ;
- **aucune revendication d'ABI stable**, **aucun support macOS x86-64 / Linux
  ARM64 / Android / iOS / navigateur WASM**
  ([`../qualification/PLATFORMS-MATRIX.md`](../qualification/PLATFORMS-MATRIX.md)
  §2).

## 11. Où lire l'état réel

| Question | Document (source de vérité) |
|---|---|
| État des 13 gates T0–T12 | [`../roadmap/CORE-1.0-GATE-MATRIX.md`](../roadmap/CORE-1.0-GATE-MATRIX.md) **§2** — au 2026-11-13 : **4 `PASS` / 7 `PARTIAL` / 2 `BLOCKED`** (T4=T12=humains) |
| État du langage, mesures du jour et limites | [`../reports/LANGUAGE-CORE-STATUS.md`](../reports/LANGUAGE-CORE-STATUS.md) §1 |
| Chemin vers la revue de production (R1–R14) | [`../roadmap/PRODUCTION-READINESS.md`](../roadmap/PRODUCTION-READINESS.md) — R14 `PARTIAL` |
| Campagnes de temps | [`../reports/LANG-TIMINGS.md`](../reports/LANG-TIMINGS.md) + [`../qualification/PERFORMANCE.md`](../qualification/PERFORMANCE.md) |
| LSP | [`../qualification/LSP.md`](../qualification/LSP.md) |
| VS Code | [`../qualification/VSCODE.md`](../qualification/VSCODE.md) |
| FFI / `unsafe` | [`../ffi/SAFETY.md`](../ffi/SAFETY.md) (D7, `PROPOSED`/`PENDING_HUMAN_APPROVAL`) |
| Installation et provenance | [`../release/INSTALL.md`](../release/INSTALL.md), [`../release/PROVENANCE.md`](../release/PROVENANCE.md) |
| Plateformes | [`../qualification/PLATFORMS-MATRIX.md`](../qualification/PLATFORMS-MATRIX.md) |
| Décisions qui attendent un humain | [`../../DECISIONS_REQUIRED.md`](../../DECISIONS_REQUIRED.md) |
| Rapport de la dernière vague de consolidation | [`../reports/GOAL-CONSOLIDATION-2-REPORT.md`](../reports/GOAL-CONSOLIDATION-2-REPORT.md) |

Règle de lecture : un `SKIP`, un `NOT_RUN`, un `PARTIAL` ou un `STALE` **n'est
jamais** un `PASS`. Les chiffres ne valent que pour la commande, la machine et le
SHA cités. Aucun document ne dit « production ready ».

## 12. Ce que ce guide ne prouve pas — `NOT_RUN` consolidés

- **Non exécuté dans cette passe** : installation Linux/macOS, harnais VS Code,
  `watch`, `test --filter`, profils `pure`, cible `wasm32-wasip1`, LSP en profil
  release, tout `cargo build`/`cargo test` du workspace, toute mesure de
  performance. Ces points sont `NOT_RUN`, pas « verts ».
- **Mesures non rejouées** : les chiffres de §10.4, §10.5 et des documents cités
  proviennent de leurs passes datées ; ils ne sont pas re-mesurés ici.
- **Portée machine** : Windows 11 x64 uniquement pour les commandes exécutées ;
  un seul SHA (`214bf5e`) et un seul binaire par profil.
- **Binaire utilisé** : `target/debug/ravum-lang.exe` (sauf mention), construit
  avant cette passe ; un binaire reconstruit depuis un autre arbre peut différer.
- **Aucune garantie de non-régression** n'est déduite de ce guide.
