# Provenance des artefacts `ravum-lang`

> **Statut : candidate, non ratifiée.** Ce document distingue ce qui est **effectivement
> observé** de ce qui est **affirmé sans preuve**. Aucune signature de release, aucune
> attestation de build et aucune reproductibilité **inter-machines** ne sont revendiquées.
> La reproductibilité locale mesurée (deux racines, même contenu) est décrite dans
> [`../qualification/REPRODUCIBILITY.md`](../qualification/REPRODUCIBILITY.md) ; elle ne
> couvre pas un artefact publié. Les statuts suivent `AGENTS.md` : `TESTED`, `ASSUMED`,
> `UNKNOWN`, `NOT_IMPLEMENTED`, `PENDING` ne sont pas `PROVED`.

## 1. Ce qui est enregistré par l'installateur

À chaque installation, `ravum-install.json` (UTF-8, sans BOM) contient :

| Champ | Provenance | Vérifié par le script |
|---|---|---|
| `version`, `package_version` | sortie de `ravum-lang --version` | oui (format contrôlé) |
| `git_commit` | `git rev-parse HEAD` | non (`UNKNOWN` si git absent) |
| `git_dirty` | `git status --porcelain` non vide | oui (avertissement si `true`) |
| `installed_at_utc` | horloge locale → UTC | — |
| `sha256` | `Get-FileHash`/`sha256sum` du binaire **construit** | oui, re-vérifié après copie |
| `rustc` | `rustc -V` | non (`UNKNOWN` si absent) |
| `platform`, `arch` | OS et architecture de la machine | — |
| `profile`, `locked` | constantes du script (`release`, `true`) | — |

Le manifeste décrit **l'installation locale**, pas un artefact publié.

## 2. Statut des propriétés (2026-11-09, Windows 11, rustc 1.95.0)

| Propriété | Statut | Preuve / limite |
|---|---|---|
| Le binaire installé correspond au SHA-256 du manifeste | **TESTED** | hash recalculé après copie, identique (`0b324e1d…6589` lors du test complet) |
| `--version` s'exécute après installation | **TESTED** | `ravum-lang 0.1.0 (edition core-1.0-draft.1)`, exit 0 |
| Désinstallation complète et idempotente | **TESTED** | binaire + manifeste + `bin` supprimés ; second passage exit 0 |
| `PATH` utilisateur ajouté une seule fois puis retiré | **TESTED** | 1 occurrence après deux installations ; `PATH` restauré à l'identique |
| Build `--locked` reproductible **à arbre identique** | **TESTED** (procédure locale) | `scripts/repro-check.ps1` : binaires identiques entre deux racines et deux `target/`, à contenu et drapeaux identiques — voir [`../qualification/REPRODUCIBILITY.md`](../qualification/REPRODUCIBILITY.md) ; des arbres différents produisent des hash différents (attendu, voir §3) |
| Build reproductible entre machines | **UNKNOWN** | non testé ; aucun `SOURCE_DATE_EPOCH`, aucun flags de reproductibilité |
| Binaire construit depuis un commit propre | **NON PROUVÉ** | `git_dirty: true` lors des tests (autres travaux en cours) |
| Absence d'accès réseau du build | **ASSUMED** | par conception (`--locked`, cache local) ; aucune capture réseau réalisée |
| Dépendances conformes à `Cargo.lock` | **ASSUMED** | Cargo vérifie les checksums des `.crate` ; pas d'audit indépendant |
| Signature de release | **mécanisme TESTED, clé absente** | `scripts/attest-release.ps1` : signature/vérification `sshsig` (namespace `ravum-release`) ; un artefact altéré est **rejeté** ([`../qualification/SIGNATURES.md`](../qualification/SIGNATURES.md)). Aucune clé de projet, aucun `allowed_signers` publié (D9) |
| SBOM généré | **TESTED** | `scripts/sbom.ps1` / `sbom.sh` : SPDX 2.3 déterministe ([`SBOM.md`](SBOM.md)) ; le fichier **n'est pas signé** |
| Attestation de build (SLSA ou équivalent) | **NOT_IMPLEMENTED** | aucun builder identifiable, aucune provenance signée |
| Lien cryptographique binaire ↔ SBOM | **NON PROUVÉ** | le SBOM décrit `Cargo.lock`, pas le fichier installé |
| Licence du code Ravum | **DECIDED** | `MIT OR Apache-2.0` ([`../../LICENSE-DECISION.md`](../../LICENSE-DECISION.md), 23/23 membres) |

## 3. Non-reproductibilité observée

Trois binaires `ravum-lang.exe` construits à des moments différents sur la même machine :

| Observé à | Arbre | SHA-256 |
|---|---|---|
| 16:02 UTC | modifications en cours (autres workers) | `502905164e106c21f3a93a6728ee608078d776f571cc8b5d62f9f4094fa74062` |
| 17:26 UTC | après correction d'une erreur de compilation | `a25a43a2450677b55f9b7d6884e6fac19ad5ed25c8e6409c5ebdb659e5b2f441` |
| 17:28 UTC | arbre encore modifié | `0b324e1d5d048ee695f2fbf9a98065e10683cacdd9e2f76ef2d1fc77c6256589` |

Ces différences ne sont **pas** un défaut de l'installateur : elles montrent que
l'empreinte reflète l'**arbre de travail exact**, pas un commit. Un artefact de release
exige un arbre propre et une procédure de build figée, qui n'existent pas encore.

## 4. Modèle de confiance (ce que l'utilisateur doit croire)

L'installateur **ne peut pas** vérifier l'origine du code : il n'existe ni signature, ni
certificat, ni dépôt d'artefacts signés. Il fait confiance :

1. au dépôt source local (branche et commit indiqués) ;
2. à la toolchain Rust installée (`rustc 1.95.0`) ;
3. au cache Cargo local pour les dépendances (`--locked`) ;
4. à l'utilisateur qui lance le script avec ses droits (installation **sans admin**).

Il **n'écrit pas** dans le `PATH` système, ne crée aucun service, aucun lancement automatique,
aucune journalisation distante. Le seul effet hors du préfixe est le `PATH` **utilisateur**,
et uniquement avec `-AddToPath` (PowerShell) ou par une ligne affichée que l'utilisateur
colle lui-même (POSIX).

## 5. Reproduction locale (procédure)

```powershell
git -C D:\App\Ravum rev-parse HEAD
cargo build -p ravum-lang-cli --release --locked
Get-FileHash target\release\ravum-lang.exe -Algorithm SHA256
pwsh -File scripts/install.ps1 -Prefix <DIR> -SkipBuild
Get-Content <DIR>\ravum-install.json
```

```sh
git -C "$PWD" rev-parse HEAD
cargo build -p ravum-lang-cli --release --locked
sha256sum target/release/ravum-lang        # ou shasum -a 256
bash scripts/install.sh --prefix <DIR> --skip-build
cat <DIR>/ravum-install.json
```

Comparer `sha256` du manifeste, le hash du fichier installé et celui du binaire construit :
les trois doivent être **identiques** sur la même machine et le même arbre. Un écart est un
défaut à instruire, pas une tolérance.

## 6. Conditions minimales avant de qualifier un artefact de « release »

Ces conditions ne sont **pas toutes** remplies aujourd'hui ; l'état de chacune est indiqué
pour que l'absence soit explicite (voir [RELEASE-CHECKLIST.md](RELEASE-CHECKLIST.md)) :

1. arbre **propre** au commit taggé (`git_dirty: false`) — **non rempli** (aucun tag) ;
2. build exécuté par un environnement identifié (CI ou machine dédiée) — **rempli côté
   Linux** (`linux-qual.yml`), **pas côté Windows** ;
3. hash du binaire publié et archivé avec le commit et la toolchain — **non rempli**
   (aucune release) ;
4. SBOM généré avec horodatage fixe, archivé à côté du binaire — SBOM **généré**, mais
   **non archivé** avec un artefact publié ;
5. décision de licence appliquée (`LICENSE`, champs `license` dans `Cargo.toml`, notices) —
   **remplie** (`MIT OR Apache-2.0`, 23/23) ;
6. procédure de rollback testée sur un poste propre — **non remplie** (`NOT_RUN`) ;
7. au moins un test d'installation sur une **seconde** plateforme (Linux ou macOS natif) —
   **remplie** pour Linux (job `install` de `linux-qual.yml`) ; macOS `NOT_RUN` ;
8. si une revendication de sécurité est faite : signature/attestation et vérification
   documentée — **mécanisme** disponible et qualifié, **clé de projet absente** (D9).

## 7. Ce que ce document n'est pas

- pas une attestation de build ;
- pas une preuve de non-compromission de la toolchain ni des dépendances ;
- pas un avis juridique sur les licences ;
- pas une garantie que le binaire installé correspond au code affiché dans le dépôt
  (`git_dirty` et absence de signature l'interdisent).
