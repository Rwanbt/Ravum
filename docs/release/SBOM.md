# SBOM SPDX 2.3 — génération et limites

> **Statut : candidate, non ratifiée.** Décrit `scripts/sbom.ps1`, `scripts/sbom.sh` et
> `tools/sbom/sbom.mjs` sur la branche `feat/lang-core-1.0-rc`, avec les valeurs observées
> le **2026-11-09**. Un SBOM n'est **pas** une preuve de conformité ni une attestation.

## 1. Objet

Le SBOM décrit **ce qui entre dans le workspace Ravum** : ses 22 membres et les dépendances
résolues par `Cargo.lock`. Il est destiné à l'inventaire (réutilisation, audit de licences,
réponse à incident), pas à certifier la chaîne de build.

Format : **SPDX 2.3 JSON**, `dataLicense: CC0-1.0`, `SPDXID: SPDXRef-DOCUMENT`.
Les composants portent un identifiant `SPDXRef-Package-<nom>-<version>`, un
`externalRef` purl `pkg:cargo/<nom>@<version>`, et des relations `DEPENDS_ON` issues du
graphe `cargo metadata --locked`.

## 2. Génération

```powershell
# Windows (défaut : target\sbom\ravum.spdx.json, horodatage courant)
pwsh -File scripts/sbom.ps1

# SBOM reproductible octet pour octet
pwsh -File scripts/sbom.ps1 -Output C:\tmp\ravum.spdx.json -Timestamp 1970-01-01T00:00:00Z
```

```sh
# Linux / macOS / Git Bash
bash scripts/sbom.sh
bash scripts/sbom.sh --output target/sbom/ravum.spdx.json --timestamp 1970-01-01T00:00:00Z
```

| Option (ps1 / sh) | Effet |
|---|---|
| `-Output` / `--output` | fichier de sortie (défaut : `target/sbom/ravum.spdx.json`) |
| `-Timestamp` / `--timestamp` | horodatage ISO-8601 UTC fixe (neutralise le temps) |
| `-Name` / `--name` | nom du document SPDX (défaut : `ravum-workspace`) |
| `-RepoRoot` / `--repo-root` | racine du dépôt (défaut : parent du script) |

Le générateur Node lit la sortie de :

```sh
cargo metadata --format-version 1 --locked
```

Aucun réseau n'est utilisé par le générateur ; `--locked` interdit la mise à jour de
`Cargo.lock`. Node.js **standard uniquement** (`node:fs`, `node:path`, `node:crypto`) :
aucune dépendance npm, aucune installation.

## 3. Déterminisme

Deux exécutions sur le **même** `cargo metadata` et le **même** horodatage produisent des
fichiers identiques octet pour octet :

| Exécution | SHA-256 du fichier |
|---|---|
| `sbom.ps1 --timestamp 1970-01-01T00:00:00Z` | `68826ed4c082062f392b34d6d75b4795e0fe5e6dfbc50859f24af3cb936fdc7c` |
| seconde exécution identique | `68826ed4c082062f392b34d6d75b4795e0fe5e6dfbc50859f24af3cb936fdc7c` |
| `sbom.sh --timestamp 1970-01-01T00:00:00Z` | `68826ed4c082062f392b34d6d75b4795e0fe5e6dfbc50859f24af3cb936fdc7c` |
| `SOURCE_DATE_EPOCH=0` (ps1) | `68826ed4c082062f392b34d6d75b4795e0fe5e6dfbc50859f24af3cb936fdc7c` |

Sans horodatage fixe, `creationInfo.created` vaut l'heure UTC courante et le fichier change
à chaque exécution : ce n'est pas une non-déterminisme du générateur, c'est le champ
`created`.

Ordre garanti : packages triés par `(name, version, id)`, relations triées, clés JSON
émises dans un ordre fixe. `documentNamespace` est
`https://spdx.ravum.invalid/<nom>-<commit>` — pas d'UUID aléatoire.

## 4. Contenu et correspondance des champs

| Champ SPDX | Source |
|---|---|
| `name`, `versionInfo` | `package.name`, `package.version` (Cargo) |
| `SPDXID` | `SPDXRef-Package-<nom>-<version>` (suffixe court si collision) |
| `downloadLocation` | `NOASSERTION` (aucune URL de téléchargement affirmée) |
| `filesAnalyzed` | `false` (aucun fichier analysé) |
| `licenseDeclared` | `package.license` de Cargo, sinon `NOASSERTION` |
| `licenseConcluded` | **toujours** `NOASSERTION` (aucune conclusion) |
| `copyrightText`, `supplier` | `NOASSERTION` |
| `externalRefs` | purl `pkg:cargo/<nom>@<version>` |
| `relationships` | `DESCRIBES` (document → membres), `DEPENDS_ON` (graphe `resolve.nodes`) |
| `creationInfo.created` | horodatage fixe ou courant (voir §3) |

Les membres du workspace sont distingués par `comment: "Workspace member …"`, les
dépendances par `comment: "Resolved Cargo dependency."`.

## 5. Validation

```powershell
$doc = Get-Content -Raw target\sbom\ravum.spdx.json | ConvertFrom-Json
$doc.spdxVersion                       # SPDX-2.3
$doc.packages.Count                    # 63 au 2026-11-09
$doc.relationships.Count               # 165
($doc.relationships | Where-Object relationshipType -eq 'DEPENDS_ON').Count   # 143
```

```sh
node -e 'const d=require("./target/sbom/ravum.spdx.json"); console.log(d.spdxVersion, d.packages.length)'
```

Valeurs observées le 2026-11-09 : **63 packages** (22 membres du workspace + 41 dépendances),
**165 relations** (143 `DEPENDS_ON` + 22 `DESCRIBES`), **22** `licenseDeclared: NOASSERTION`
(les 22 membres du workspace n'ont pas de champ `license` dans `Cargo.toml`), document
namespace `https://spdx.ravum.invalid/ravum-workspace-8fd154726ae9`.

## 6. Limites explicites

1. **Pas de niveau fichier** : `filesAnalyzed: false`, aucun `File`/`PackageVerificationCode`,
   aucun checksum d'artefact. Le SBOM décrit des **composants**, pas un binaire.
2. **Pas de conclusion de licence** : `licenseConcluded` est toujours `NOASSERTION`. Les
   licences déclarées proviennent de Cargo et peuvent être absentes ou approximatives.
3. **Pas de signature ni d'attestation** : le fichier peut être modifié sans détection par un
   consommateur. Aucun `PROVENANCE` signé (voir [PROVENANCE.md](PROVENANCE.md)).
4. **Pas de données de vulnérabilité** : aucun identifiant CVE/OSV, aucune jointure avec une
   base externe. Aucun réseau n'est utilisé.
5. **Dépendances uniquement `Cargo.lock`** : pas de dépendance système, pas de `rustc`/
   `cargo` eux-mêmes, pas de Node.js, pas d'outils de build.
6. **Aucune garantie de complétude** : une dépendance introduite hors Cargo (script, binaire
   téléchargé) n'apparaît pas.
7. **Licence du workspace non décidée** : les 22 membres n'ont pas de licence déclarée, ce
   qui est un **choix en attente** (voir `LICENSE-DECISION.md`), pas un oubli du SBOM.
8. **Le SBOM ne prouve pas que le binaire a été construit à partir de ces sources** : il est
   calculé à partir de `Cargo.lock`, indépendamment de l'artefact installé.

## 7. Quand (re)générer

- avant toute remise d'un artefact à un tiers ;
- après tout changement de `Cargo.toml`/`Cargo.lock` ;
- dans le cadre d'un audit de licences (`scripts/licence-audit.ps1` reste complémentaire :
  il signale les licences fortes/non commerciales) ;
- avec un horodatage fixe pour archiver un SBOM reproductible.

Le SBOM doit être archivé **avec** le manifeste d'installation
(`ravum-install.json`) et le commit exact, jamais seul.
