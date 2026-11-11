# Installation de `ravum-lang` (Ravum Core 1.0)

> **Statut : candidate, non ratifiée.** Ce document décrit les scripts `scripts/install.ps1`
> (Windows) et `scripts/install.sh` (Linux/macOS/WSL/Git Bash) tels qu'ils existent sur la
> branche `feat/lang-core-1.0-rc`. Les valeurs observées datent du **2026-11-09** et ne sont
> pas des garanties de release.
>
> **Deux binaires différents existent dans le projet :**
> - `ravum-lang` — le **compilateur du langage Ravum** (crate `ravum-lang-cli`). C'est ce que
>   ces scripts installent.
> - `ravum` — l'**orchestrateur Rust historique** (snapshots, overlays, daemon). Il n'est
>   **pas** installé par ces scripts et ne compile aucun fichier `.rv`.

## 1. Prérequis

| Élément | Windows | Linux / macOS |
|---|---|---|
| Shell | PowerShell 7+ (`pwsh`) | `bash` 4+ (`set -euo pipefail`), `sha256sum` ou `shasum` |
| Rust | toolchain **1.95.0** (`rust-toolchain.toml`) | idem |
| Cargo | `cargo` dans `PATH` | idem |
| Dépôt | `D:\App\Ravum` (ou clone local) | clone local |
| Réseau | **non requis** : le build utilise `--locked` et le cache local | idem |
| Node.js | seulement pour `scripts/sbom.ps1` (≥ 18) | seulement pour `scripts/sbom.sh` |
| Droits | **aucun admin** : installation dans le profil utilisateur | idem |

Le script vérifie `Cargo.toml`, `cargo`, le binaire construit et le manifeste. Il n'installe
ni ne modifie aucun crate, workflow ou fichier du dépôt (hors `target/`).

## 2. Installation sous Windows

```powershell
# Depuis la racine du dépôt
pwsh -File scripts/install.ps1
```

Effet :

1. `cargo build -p ravum-lang-cli --release --locked` (échec ⇒ arrêt, aucune installation) ;
2. copie du binaire dans un **staging** puis remplacement atomique ;
3. écriture de `ravum-install.json` (voir §4) ;
4. **aucune** modification du `PATH` sauf avec `-AddToPath`.

| Option | Effet |
|---|---|
| `-Prefix <DIR>` | préfixe d'installation (défaut : `%LOCALAPPDATA%\Ravum`) |
| `-AddToPath` | ajoute `<Prefix>\bin` au `PATH` **utilisateur** (idempotent) |
| `-SkipBuild` | utilise `target\release\ravum-lang.exe` déjà construit |
| `-Uninstall` | supprime binaire + manifeste + `bin` vide (idempotent) |
| `-RemoveFromPath` | avec `-Uninstall` : retire `<Prefix>\bin` du `PATH` utilisateur |

Exemples :

```powershell
pwsh -File scripts/install.ps1 -Prefix C:\Tools\Ravum -AddToPath
pwsh -File scripts/install.ps1 -Prefix C:\Tools\Ravum -Uninstall -RemoveFromPath
```

Un **nouveau terminal** est nécessaire pour que le `PATH` soit pris en compte. Le script ne
vérifie pas la signature du binaire (il n'y en a pas ; voir [PROVENANCE.md](PROVENANCE.md)).

## 3. Installation sous Linux / macOS / Git Bash

> **État de qualification :** Linux x86-64 est exercé en CI (`linux-qual.yml`, job
> `install`) ; **macOS n'a jamais été exécuté** (`NOT_RUN`, voir
> [`../qualification/PLATFORMS-MATRIX.md`](../qualification/PLATFORMS-MATRIX.md)). La
> présence de branches `darwin` dans le script n'est pas une preuve de support.

```sh
# Depuis la racine du dépôt
bash scripts/install.sh
```

| Option | Effet |
|---|---|
| `--prefix DIR` | préfixe (défaut : `$RAVUM_PREFIX` ou `$HOME/.local/share/ravum`) |
| `--add-to-path` | n'écrit **rien** : affiche la ligne `export PATH="DIR/bin:$PATH"` |
| `--skip-build` | utilise `target/release/ravum-lang` déjà construit |
| `--uninstall` | supprime binaire + manifeste (idempotent) |
| `--remove-from-path` | avec `--uninstall` : affiche les instructions de retrait |

```sh
bash scripts/install.sh --prefix "$HOME/.local/share/ravum" --add-to-path
bash scripts/install.sh --prefix "$HOME/.local/share/ravum" --uninstall --remove-from-path
```

Sous **Git Bash / MSYS**, un chemin Windows (`C:/Users/...`) est converti via `cygpath` ;
sans `cygpath`, le script refuse le chemin (code 2) plutôt que d'écrire au mauvais endroit.

Le binaire est installé avec le bit exécutable (chmod 755). Le script ne modifie **aucun** fichier de profil (`~/.bashrc`, `~/.zshrc`, `~/.profile`) :
la ligne `export PATH=...` est affichée et doit être ajoutée manuellement.

## 4. Manifeste `ravum-install.json`

Écrit à la racine du préfixe, en UTF-8 sans BOM. Exemple réel (Windows, 2026-11-09) :

```json
{
  "schema_version": 1,
  "name": "ravum-lang",
  "version": "ravum-lang 0.1.0 (edition core-1.0-draft.1)",
  "package_version": "0.1.0",
  "git_commit": "8fd154726ae97c172c8be55fce811cb3f499653f",
  "git_dirty": true,
  "installed_at_utc": "2026-11-09T16:28:56Z",
  "sha256": "0b324e1d5d048ee695f2fbf9a98065e10683cacdd9e2f76ef2d1fc77c6256589",
  "rustc": "rustc 1.95.0 (59807616e 2026-04-14)",
  "platform": "windows",
  "arch": "AMD64",
  "profile": "release",
  "locked": true,
  "binary": "bin/ravum-lang.exe",
  "prefix": "C:\\Users\\<user>\\AppData\\Local\\Temp\\ravum-install-full-…",
  "installer": "scripts/install.ps1"
}
```

Sous Linux/macOS, `platform` vaut `linux`/`darwin`, `arch` `x86_64`/`arm64`, `binary`
`bin/ravum-lang`, et `installer` `scripts/install.sh`.

| Champ | Sens |
|---|---|
| `version` | sortie complète de `ravum-lang --version` |
| `package_version` | version du crate (`0.1.0`) |
| `git_commit` | `git rev-parse HEAD` au moment de l'installation (`UNKNOWN` si git absent) |
| `git_dirty` | `true` si l'arbre de travail contenait des modifications non commitées |
| `sha256` | empreinte du binaire **installé** (vérifiée après copie) |
| `rustc` | sortie de `rustc -V` |
| `locked` | `true` : build avec `--locked` |

> `git_dirty: true` signifie que le binaire **ne correspond pas exactement** au commit
> enregistré. Ne pas présenter un tel binaire comme un artefact de release (voir
> [PROVENANCE.md](PROVENANCE.md)).

## 5. Vérification

```powershell
# Windows
& "$env:LOCALAPPDATA\Ravum\bin\ravum-lang.exe" --version
# attendu : ravum-lang 0.1.0 (edition core-1.0-draft.1)

Get-Content "$env:LOCALAPPDATA\Ravum\ravum-install.json"
(Get-FileHash "$env:LOCALAPPDATA\Ravum\bin\ravum-lang.exe" -Algorithm SHA256).Hash.ToLower()
```

```sh
# Linux / macOS / Git Bash
"$HOME/.local/share/ravum/bin/ravum-lang" --version
cat "$HOME/.local/share/ravum/ravum-install.json"
sha256sum "$HOME/.local/share/ravum/bin/ravum-lang"   # ou shasum -a 256
```

Le SHA-256 affiché doit être **identique** à `sha256` du manifeste (le script le vérifie
déjà après installation ; cette commande permet de le re-vérifier plus tard).

Diagnostic de la toolchain (si la commande existe sur la version installée) :

```sh
ravum-lang doctor --json
```

`doctor` peut signaler `rustc` absent ou un environnement non supporté ; cela n'indique pas
que l'installation du binaire a échoué.

## 6. Désinstallation

```powershell
pwsh -File scripts/install.ps1 -Uninstall
# avec nettoyage du PATH utilisateur :
pwsh -File scripts/install.ps1 -Uninstall -RemoveFromPath
```

```sh
bash scripts/install.sh --uninstall
bash scripts/install.sh --uninstall --remove-from-path   # affiche les instructions
```

La désinstallation est **idempotente** : relancée sur un préfixe absent, elle affiche
« rien à désinstaller » et sort en `0`. Elle ne supprime **que** `<Prefix>\bin\ravum-lang*` et
`<Prefix>\ravum-install.json` ; aucun autre fichier utilisateur n'est touché.

## 7. Dépannage

| Symptôme | Cause probable | Action |
|---|---|---|
| `cargo build a echoue (code 101)` | arbre de travail cassé (édition concurrente), ou `--locked` incompatible avec `Cargo.lock` | `cargo build -p ravum-lang-cli --release --locked` à la main, lire l'erreur ; ne pas contourner `--locked` |
| `binaire en cours d'utilisation` | un `ravum-lang` tourne (daemon/test) | fermer le processus puis relancer |
| `node introuvable` | SBOM, et `run --target wasm32-wasip1` | installer Node ≥ 18 (SBOM) / ≥ 22 pour WASI, ou utiliser un autre poste |
| `PATH` inchangé dans le terminal courant | le `PATH` utilisateur n'est lu qu'au démarrage | ouvrir un nouveau terminal |
| Script `.ps1` bloqué par la stratégie d'exécution | PowerShell restreint | décision humaine : `pwsh -File` avec une stratégie adaptée, ou exécution manuelle |
| `rustc` absent | toolchain non installée | installer Rust 1.95.0 (`rustup toolchain install 1.95.0`) |
| Build très long | première compilation du workspace | normal ; les builds suivants sont incrémentaux |

## 8. Ce que l'installateur ne fait pas

- aucune signature n'est produite : le **mécanisme** existe et est qualifié
  ([`SIGNATURES.md`](../qualification/SIGNATURES.md)), mais aucune clé de projet ni
  `allowed_signers` n'existe (D9) ;
- aucune vérification d'intégrité du dépôt source (hors `--locked`) ;
- aucune écriture dans le `PATH` **système**, aucune clé de registre, aucun service ;
- aucun accès réseau **volontaire** : le build `--locked` utilise le cache local ; l'absence
  totale d'accès réseau n'est pas capturée (statut `ASSUMED`, [`PROVENANCE.md`](PROVENANCE.md) §2) ;
- aucune mise à jour automatique, aucun rollback automatique après installation réussie ;
- aucune décision de licence à prendre : elle est **décidée** (`MIT OR Apache-2.0`, 23/23).

## 9. Valeurs observées (2026-11-09, Windows 11, PowerShell 7)

| Mesure | Valeur |
|---|---|
| `ravum-lang --version` | `ravum-lang 0.1.0 (edition core-1.0-draft.1)` |
| `rustc -V` | `rustc 1.95.0 (59807616e 2026-04-14)` |
| commit enregistré | `8fd154726ae97c172c8be55fce811cb3f499653f` |
| `git_dirty` à l'installation testée | `true` (arbres modifiés par d'autres travaux en cours) |
| taille du binaire | 2 566 144 octets (build du 2026-11-09) |
| SHA-256 observé (installation complète) | `0b324e1d5d048ee695f2fbf9a98065e10683cacdd9e2f76ef2d1fc77c6256589` |
| SHA-256 observé (build antérieur, arbre différent) | `502905164e106c21f3a93a6728ee608078d776f571cc8b5d62f9f4094fa74062` |

Ces empreintes ne sont **pas** des empreintes de release : elles dépendent de l'arbre de
travail exact (voir [PROVENANCE.md](PROVENANCE.md)).

