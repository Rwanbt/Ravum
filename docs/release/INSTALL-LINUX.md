# Contrat public de l'installateur Linux (`ravum-lang`)

> **Statut : `PROPOSED` — candidate, non ratifiée · 2026-11-14.**
> Ce document décrit le contrat **que `scripts/install.sh` implémente réellement**
> (source de vérité) et que `scripts/install-contract.sh` vérifie. Il n'annonce
> aucune release, aucun artefact signé, aucune maturité « production ».
>
> **Exécution du contrat : `NOT_RUN` localement, `PENDING` en CI.** Le poste de
> développement est Windows ; l'hôte du contrat doit être Linux (§1). L'exécution
> réelle est l'étape ajoutée au job `install` de
> [`.github/workflows/linux-qual.yml`](../../.github/workflows/linux-qual.yml).
> Tant qu'aucun run vert n'est consigné ici, l'état est `PENDING`, **pas** `PASS`.
> Le job `install` existant (installation sous préfixe temporaire puis
> `--version`) est vert au run `43562818570` (SHA `878e212`) ; c'est une preuve
> de l'installateur, pas du contrat élargi décrit ici
> ([`LINUX-QUALIFICATION.md`](../qualification/LINUX-QUALIFICATION.md) §12).

## 1. Périmètre

| Plateforme | État vis-à-vis de ce contrat | Preuve / limite |
|---|---|---|
| **Linux x86-64, libc glibc, image `ubuntu-latest`** | **couvert (périmètre)** — exécution `PENDING` jusqu'au premier run vert | installation, manifeste, SHA-256, désinstallation, erreurs d'usage. `scripts/install.sh` + `scripts/install-contract.sh` ; étape CI dans `linux-qual.yml`. L'image n'est **pas** épinglée : `rustc`/Node le sont, pas `ubuntu-latest` ([`LINUX-QUALIFICATION.md`](../qualification/LINUX-QUALIFICATION.md) §7.8) |
| **Linux x86-64 musl** | **non couvert** (`NOT_RUN`) | aucune cible musl dans `rust-toolchain.toml` ni dans les workflows ; aucune compilation ni installation musl n'a été mesurée |
| **Linux ARM64** | **non couvert** (`NOT_RUN`) | le CLI refuse `--target aarch64-unknown-linux-gnu` (exit 2) et aucun job CI ARM n'existe ([`PLATFORMS-MATRIX.md`](../qualification/PLATFORMS-MATRIX.md) §2 et §2.3) |
| **macOS** | **non couvert** par ce contrat (`NOT_RUN` pour l'installation) | `scripts/install.sh` contient des branches `darwin` mais n'a jamais tourné sur macOS ([`INSTALL.md`](INSTALL.md) §3, [`PLATFORMS-MATRIX.md`](../qualification/PLATFORMS-MATRIX.md) §2). `macos-qual.yml` (Tier 2) exécute tests et conformance, **pas** l'installateur |
| **Windows** | **hors périmètre** | contrat distinct : `scripts/install.ps1`, exercé par `.github/workflows/qualification-evidence.yml` |

Une compilation croisée n'est pas un support : seule une exécution sur l'hôte
compte. Ce document ne convertit aucun `NOT_RUN` en `PASS`.

## 2. Prérequis

| Élément | Valeur | Source |
|---|---|---|
| Shell | `bash` 4+ (`set -euo pipefail`) | `scripts/install.sh` |
| Rust | canal épinglé **`1.95.0`** | [`../../rust-toolchain.toml`](../../rust-toolchain.toml) |
| Cargo | `cargo` dans `PATH`, cache local des dépendances | `scripts/install.sh` |
| Git | `git` dans `PATH` ; le commit est enregistré (`UNKNOWN` si absent) | `scripts/install.sh` |
| Empreinte | `sha256sum` **ou** `shasum -a 256` | `scripts/install.sh` (repli documenté) |
| Contrat seul | `python3` (analyse du manifeste, aucun `jq`) | `scripts/install-contract.sh` |
| Droits | **aucun admin** : écriture limitée au préfixe choisi | `scripts/install.sh` |
| Réseau | voir §9 | — |

`cargo` absent : l'installation échoue en `1` (le build est obligatoire sans
`--skip-build`). `rustc` absent **n'échoue pas** l'installateur : le champ
`rustc` vaut alors `UNKNOWN`. Le contrat exige en plus que `rustc -V` corresponde
au canal lu dans `rust-toolchain.toml` (canal non recopié en dur dans le script) ;
si la toolchain est inobservable, cet invariant est marqué `NOT_RUN` et n'est pas
compté — en CI, le build aurait déjà échoué avant.

## 3. Installation

Préfixe par défaut : **`$HOME/.local/share/ravum`** (`$RAVUM_PREFIX` prioritaire).
Le binaire va dans `<préfixe>/bin/ravum-lang`, le manifeste à la racine du
préfixe.

```sh
# Depuis la racine du dépôt
bash scripts/install.sh

# Préfixe explicite
bash scripts/install.sh --prefix "$HOME/.local/share/ravum"
bash scripts/install.sh --prefix "$PWD/target/install-test"

# Binaire déjà construit (aucun cargo lancé)
cargo build -p ravum-lang-cli --release --locked
bash scripts/install.sh --prefix "$HOME/.local/share/ravum" --skip-build
```

| Option | Effet |
|---|---|
| `--prefix DIR` / `--prefix=DIR` | préfixe d'installation (défaut : `$RAVUM_PREFIX`, sinon `$HOME/.local/share/ravum`) |
| `--skip-build` | ne lance pas `cargo` ; exige `target/release/ravum-lang` |
| `--add-to-path` | n'écrit **rien** : affiche la ligne `export PATH="DIR/bin:$PATH"` |
| `--uninstall` | supprime binaire et manifeste (idempotent, §7) |
| `--remove-from-path` | avec `--uninstall` : affiche les instructions de retrait |
| `-h`, `--help` | aide, code `0` |

Le script **ne modifie aucun** fichier de profil (`~/.bashrc`, `~/.zshrc`,
`~/.profile`) et n'écrit jamais dans un `PATH` système. Sous Git Bash/MSYS un
chemin Windows est converti par `cygpath` ; sans `cygpath`, il est refusé (code
`2`) plutôt qu'écrit au mauvais endroit. Ce cas n'appartient pas au périmètre
Linux de ce contrat.

## 4. Publication atomique et rollback

L'installateur ne copie pas directement sur la destination : il **prépare**,
puis **remplace**.

1. `mkdir -p` du préfixe ; staging créé **dans le préfixe** via
   `mktemp -d "$PREFIX/.staging.XXXXXX"` (même système de fichiers, donc
   `mv` est un `rename` local) ;
2. copie du binaire construit et écriture du manifeste **dans le staging** ;
3. sauvegarde de l'installation précédente (`bin/ravum-lang.bak`,
   `ravum-install.json.bak`) si elle existe ;
4. `mv -f` du binaire, puis `mv -f` du manifeste ;
5. `chmod 755` du binaire, puis **re-calcul du SHA-256 installé** et comparaison
   avec le SHA-256 construit ;
6. suppression des sauvegardes ; le staging est supprimé par `trap EXIT`.

**Fail-closed** : si le build, la copie, l'un des remplacements, le `chmod` ou la
comparaison de SHA échoue, l'installation précédente est restaurée (`rollback`)
et le script sort en `1`. Une installation réussie ne laisse ni `.staging.*` ni
`.bak` (le contrat le vérifie).

**Limites, à ne pas surinterpréter :**

- l'atomicité porte sur **chaque fichier** (`rename`), pas sur une transaction
  multi-fichiers : une interruption brutale (SIGKILL, coupure) **entre** les deux
  `mv` peut laisser un binaire neuf et un manifeste ancien, avec les `.bak` sur
  place. Aucun test de coupure brutale n'a été exécuté (`NOT_RUN`) ;
- aucune synchronisation disque (`fsync`) n'est demandée : la garantie est une
  garantie de **remplacement**, pas de durabilité après coupure de courant ;
- le rollback est exercé par le code de l'installateur, pas par un test
  d'injection de panne dans la CI. Ce qui est testé en CI : installation,
  invariants, désinstallation, codes d'erreur.

## 5. Manifeste `ravum-install.json`

Écrit à la racine du préfixe, **UTF-8 sans BOM**, une clé par champ. Les valeurs
sont celles de la machine d'installation ; le manifeste décrit une
**installation locale**, pas un artefact publié ([`PROVENANCE.md`](PROVENANCE.md) §1).

| Champ | Forme | Invariant vérifié par `install-contract.sh` |
|---|---|---|
| `schema_version` | entier `1` | égal à `1` (entier, pas booléen) |
| `name` | chaîne | `"ravum-lang"` |
| `version` | chaîne | **exactement** la sortie de `<préfixe>/bin/ravum-lang --version` |
| `package_version` | chaîne | second jeton de `version`, forme semver (`X.Y.Z`, suffixe pré-release/build toléré) |
| `git_commit` | 40 hex | `git rev-parse HEAD` du dépôt quand git est présent ; sinon `UNKNOWN` |
| `git_dirty` | booléen JSON | état réel de l'arbre : `true` si `git status --porcelain` est non vide |
| `installed_at_utc` | `AAAA-MM-JJThh:mm:ssZ` | horodatage UTC analysable |
| `sha256` | 64 hex minuscules | SHA-256 du binaire **installé** = SHA-256 du binaire **construit** |
| `rustc` | chaîne | sortie de `rustc -V` ; commence par le canal épinglé (`rustc 1.95.0`) |
| `platform` | chaîne | `"linux"` = `uname -s` en minuscules |
| `arch` | chaîne | `uname -m` de l'hôte |
| `profile` | chaîne | `"release"` |
| `locked` | booléen JSON | **`true`** : build avec `--locked` |
| `binary` | chaîne | `"bin/ravum-lang"`, et le chemin résolu est le binaire installé |
| `prefix` | chaîne | préfixe exact demandé |
| `installer` | chaîne | `"scripts/install.sh"` |

Invariants structurels supplémentaires :

- l'ensemble des clés est **exactement** celui ci-dessus : une clé ajoutée sans
  mise à jour de ce document et du contrat fait échouer la vérification ;
- à la racine du préfixe, seuls `bin/` et `ravum-install.json` existent après
  installation (aucun staging ni sauvegarde résiduel) ;
- le binaire installé est un fichier régulier **exécutable** ;
- `git_dirty: true` signifie que le binaire **ne correspond pas exactement** au
  commit enregistré : un tel binaire n'est pas un artefact de release
  ([`PROVENANCE.md`](PROVENANCE.md) §1 et §3).
- en CI, le checkout est un arbre propre : le manifeste porte donc
  `git_dirty: false`. Le contrat compare le champ à l'état **observé** de
  l'arbre : un `git_dirty` qui ment sur son arbre est refusé ; un `true` sur un
  arbre réellement modifié est accepté mais qualifie une installation locale,
  pas un artefact de release.

Le manifeste ne contient **aucun secret**. Exemple de forme (valeurs de la
machine, non archivées ici) : `{"schema_version": 1, "name": "ravum-lang",
"locked": true, "profile": "release", "platform": "linux", "installer":
"scripts/install.sh", …}`.

## 6. Vérification

Vérification manuelle (l'installateur le fait déjà après copie ; ces commandes
permettent de le re-vérifier plus tard) :

```sh
prefix="$HOME/.local/share/ravum"

"$prefix/bin/ravum-lang" --version
sha256sum "$prefix/bin/ravum-lang"          # ou : shasum -a 256

python3 - "$prefix/ravum-install.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1], encoding="utf-8"))
for key in ("version", "package_version", "git_commit", "git_dirty", "sha256",
            "rustc", "platform", "arch", "profile", "locked", "binary",
            "prefix", "installer"):
    print(f"{key}: {d[key]}")
PY
```

Le SHA-256 affiché doit être **identique** à `sha256` du manifeste ; pour le
binaire installé, il doit aussi être identique au `target/release/ravum-lang`
construit sur la même machine et le même arbre ([`PROVENANCE.md`](PROVENANCE.md) §5).

Contrat automatisé (le même que la CI) :

```sh
cargo build -p ravum-lang-cli --release --locked
bash scripts/install-contract.sh --skip-build

# sans --skip-build, le script lance lui-même le build :
bash scripts/install-contract.sh
```

Le script exige un hôte **Linux** ; hors Linux il sort en `3` (`NOT_RUN`) sans
simuler de `PASS`. En CI, cette vérification est l'étape
« Public installer contract (manifest, sha256, uninstall, usage codes) » du job
`install` de `linux-qual.yml`. Forme observée de `--version` sur le binaire
release construit sur le poste Windows au commit `214bf5e` :
`ravum-lang 0.1.0 (edition core-1.0-draft.1)`. La valeur Linux n'est pas encore
mesurée (`NOT_RUN`) : le contrat compare le manifeste à la sortie réelle du
binaire, il ne code pas cette chaîne en dur.

## 7. Désinstallation

```sh
bash scripts/install.sh --uninstall
bash scripts/install.sh --uninstall --remove-from-path   # affiche les instructions PATH
```

La désinstallation est **idempotente** : relancée sur un préfixe absent, elle
affiche `rien a desinstaller` et sort en `0`. Elle ne supprime que
`<préfixe>/bin/ravum-lang` et `<préfixe>/ravum-install.json`, puis tente
`rmdir` sur `bin/` s'il est vide. Aucun autre fichier du préfixe n'est touché :
le contrat dépose un fichier témoin à la racine du préfixe et vérifie qu'il
survit. Sur POSIX, `--remove-from-path` **n'écrit rien** : il exige
`--uninstall` et affiche les lignes à retirer manuellement des fichiers de
profil.

## 8. Codes de sortie

`scripts/install.sh` : **0** succès, **1** échec, **2** usage invalide.

| Code | Cas observés / testés |
|---|---|
| `0` | installation réussie ; désinstallation réussie (y compris seconde désinstallation) ; `--help` |
| `1` | `cargo build` en échec ; binaire construit introuvable (`--skip-build` sans binaire) ; copie/staging impossible ; remplacement impossible (binaire en cours d'utilisation) ; `chmod` en échec ; SHA-256 installé différent ; `--version` du binaire en échec |
| `2` | argument inconnu ; `--prefix` sans valeur ; préfixe vide ; préfixe = racine du dépôt ; `Cargo.toml` introuvable ; chemin Windows non convertible (`cygpath` absent) ; `--remove-from-path` sans `--uninstall` |

`scripts/install-contract.sh` ajoute son propre code **3** = `NOT_RUN`
(prérequis absent : hôte non Linux, `python3` absent, installateur absent,
binaire absent avec `--skip-build`). `install.sh` ne retourne jamais `3`.

## 9. Comportement réseau

- Le build lancé par l'installateur est
  `cargo build -p ravum-lang-cli --release --locked` : `--locked` interdit la
  mise à jour de `Cargo.lock` et s'appuie sur le cache Cargo local
  ([`SBOM.md`](SBOM.md) §2).
- Le script d'installation ne contient **aucun** appel réseau explicite.
- Si le cache local ne contient pas les `.crate` verrouillés, cargo peut les
  récupérer : c'est le seul chemin réseau possible pendant une installation.
- L'absence totale d'accès réseau n'est **pas capturée** ; elle est `ASSUMED`
  par conception ([`PROVENANCE.md`](PROVENANCE.md) §2).
- Avec `--skip-build`, ni cargo ni réseau ne sont invoqués par l'installateur.

## 10. Limites explicites

1. **Pas de signature.** Aucune clé de projet, aucun `allowed_signers` publié
   (D9) ; le mécanisme `sshsig` est qualifié mais l'artefact installé n'est pas
   signé ([`SIGNATURES.md`](../qualification/SIGNATURES.md),
   [`PROVENANCE.md`](PROVENANCE.md) §2).
2. **Pas d'auto-update, pas de mise à jour, pas de rollback après installation
   réussie.** Réinstaller revient à relancer le script ; il n'existe ni canal de
   mise à jour, ni service, ni tâche planifiée.
3. **Pas de vérification de provenance hors manifeste.** Le manifeste est
   produit localement, non signé : il ne prouve pas que le binaire correspond au
   commit affiché (`git_dirty` et l'absence d'attestation de build l'interdisent,
   [`PROVENANCE.md`](PROVENANCE.md) §4 et §7). Le SBOM décrit `Cargo.lock`, pas le
   fichier installé ([`SBOM.md`](SBOM.md) §6.8).
4. **Pas d'artefact distribuable.** L'installateur construit depuis les sources
   locales ; il n'existe ni `.deb`/`.rpm`/archive, ni dépôt d'artefacts, ni
   version publiée.
5. **Atomicité partielle** (§4) : remplacement par fichier, sans `fsync`, sans
   test de coupure brutale.
6. **Pas de reproductibilité inter-machines revendiquée** : `UNKNOWN`
   ([`PROVENANCE.md`](PROVENANCE.md) §2).
7. **Périmètre plateforme étroit** : Linux x86-64 glibc sur l'image
   `ubuntu-latest` ; musl, ARM et macOS ne sont pas couverts (§1).
8. **Le contrat ne dit rien de la sémantique du compilateur.** Il vérifie
   l'installation, le manifeste et les codes de sortie ; la correction du
   langage relève de la conformance et du smoke
   ([`LINUX-QUALIFICATION.md`](../qualification/LINUX-QUALIFICATION.md) §2).

## 11. FFI et base de confiance (TCB)

L'installateur installe un **compilateur**. Les programmes qu'il produit peuvent
déclarer `extern "C"` et appeler des symboles déjà liés par défaut (CRT/libm) ;
la surface est scalaire, sans pointeur, sans `#[link]`
([`../ffi/SAFETY.md`](../ffi/SAFETY.md) §2). D7 est `PROPOSED` et
`PENDING_HUMAN_APPROVAL` : rien n'est ratifié.

Phrase de TCB, sans adoucissement (C-FFI-10 et §1 de
[`../ffi/SAFETY.md`](../ffi/SAFETY.md)) : **appeler une fonction C place cette
bibliothèque dans la base de confiance du processus.** Un programme Ravum peut
donc être corrompu par du code que Ravum n'a pas analysé. L'installation
n'ajoute ni ne retire cette limite : elle installe le binaire, elle ne vérifie
aucune bibliothèque C et ne signe rien.

## 12. Références

- [`scripts/install.sh`](../../scripts/install.sh) — installateur, source de vérité.
- [`scripts/install-contract.sh`](../../scripts/install-contract.sh) — contrat automatisé.
- [`scripts/install.ps1`](../../scripts/install.ps1) — équivalent Windows.
- [`.github/workflows/linux-qual.yml`](../../.github/workflows/linux-qual.yml) — étape CI du contrat.
- [`INSTALL.md`](INSTALL.md) — installation Windows et Linux, valeurs observées.
- [`PROVENANCE.md`](PROVENANCE.md) — ce que le manifeste prouve et ne prouve pas.
- [`SBOM.md`](SBOM.md) — inventaire des composants, hors artéfact installé.
- [`RELEASE-CHECKLIST.md`](RELEASE-CHECKLIST.md) — conditions non remplies avant une release.
- [`../qualification/LINUX-QUALIFICATION.md`](../qualification/LINUX-QUALIFICATION.md) — méthode et runs Linux.
- [`../qualification/PLATFORMS-MATRIX.md`](../qualification/PLATFORMS-MATRIX.md) — état par plateforme.

## 13. Ce que ce document n'est pas

- pas une preuve de maturité produit ni une autorisation de publication ;
- pas une attestation de build, ni une signature, ni un SBOM ;
- pas une garantie que le binaire installé correspond au code du dépôt ;
- pas un support annoncé pour musl, ARM, macOS ou Windows : chaque `NOT_RUN`
  reste un `NOT_RUN`.
