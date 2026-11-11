# Checklist de release — `ravum-lang` Core 1.0 (RC)

> **Statut : candidate, non ratifiée.** Une case cochée signifie **vérifié localement le
> 2026-11-09** avec la commande citée, pas « prouvé pour une release ». Les cases non cochées
> sont des **conditions non remplies** : ne pas publier en les présentant comme faites.

## 0. Règles

- Une case ne se cochent que sur preuve exécutée et liée à un commit/arbre identifié.
- Un résultat `UNKNOWN`, `NOT_RUN`, `ASSUMED` ou `PARTIAL` ne cochent **jamais** une case.
- Toute case décochée à la publication doit être listée dans le rapport de release avec sa
  raison (voir `docs/mvp/` pour le format des rapports honnêtes).

## 1. Source et arbre

| Case | Condition | Preuve attendue |
|---|---|---|
| [ ] | Arbre **propre** (`git status --porcelain` vide) au commit taggé | sortie de `git status` |
| [ ] | Commit taggé (tag annoté) et poussé | `git tag -v`, `git rev-parse` |
| [ ] | `Cargo.lock` commité et cohérent (`cargo metadata --locked`) | exit 0 |
| [ ] | Version de crate incrémentée et cohérente avec le tag | `Cargo.toml`, `--version` |
| [ ] | `CHANGELOG`/notes de version à jour | fichier daté |

## 2. Build

| Case | Condition | Preuve attendue |
|---|---|---|
| [ ] | Build release **`--locked`** depuis un arbre propre | `cargo build -p ravum-lang-cli --release --locked` exit 0 |
| [x] | Build release `--locked` exécuté au moins une fois sur cette machine | exit 0, 13,67 s (2026-11-09 17:26 UTC) — **arbre modifié** (`git_dirty=true`) |
| [x] | Échec de build ⇒ installateur **fail-closed** (aucune installation) | observé : `E0004` dans `ravum-lang-codegen` ⇒ `install.ps1` exit 1, préfixe vide |
| [ ] | Reproductibilité bit-à-bit démontrée (même machine, même arbre) | hashes identiques sur deux builds consécutifs **propres** |
| [ ] | Reproductibilité inter-machines démontrée | comparaison de hashes documentée |
| [ ] | Avertissements `rustc` traités ou explicitement acceptés | liste + décision |

## 3. Artefact

| Case | Condition | Preuve attendue |
|---|---|---|
| [ ] | Binaire construit depuis le commit taggé (arbre propre) | `git_dirty: false` |
| [x] | Binaire exécutable et `--version` conforme | `ravum-lang 0.1.0 (edition core-1.0-draft.1)` |
| [x] | SHA-256 du binaire enregistré (installation locale) | `0b324e1d5d048ee695f2fbf9a98065e10683cacdd9e2f76ef2d1fc77c6256589` (arbre modifié) |
| [ ] | Checksums publiés et archivés avec l'artefact | fichier `SHA256SUMS` publié |
| [ ] | Signature/attestation de l'artefact | mécanisme `attest-release` **qualifié** (accepte/rejette), mais **aucune clé de projet** ni `allowed_signers` (D9) |

## 4. Installation

| Case | Condition | Preuve attendue |
|---|---|---|
| [x] | `install.ps1` : installation complète (build inclus) dans un préfixe temporaire | exit 0, binaire + manifeste présents |
| [x] | `install.ps1` : désinstallation complète et idempotente | binaire/manifeste/`bin` supprimés ; second passage exit 0 |
| [x] | `install.ps1` : `-AddToPath` idempotent et `-RemoveFromPath` restaure le `PATH` | 1 occurrence après 2 installations ; `PATH` identique après retrait |
| [x] | `install.ps1` : manifeste JSON valide, hash cohérent avec le fichier | `sha_manifeste == sha_fichier` |
| [x] | `install.sh` : syntaxe valide (`bash -n`) | exit 0 |
| [x] | `install.sh` : installation/désinstallation dans un préfixe temporaire (Git Bash) | exit 0, manifeste parsé, binaire exécuté |
| [x] | `install.sh` : chemin Windows (`C:/…`) converti via `cygpath`, pas d'écriture hors préfixe | testé ; aucune pollution du dépôt |
| [x] | Installation testée sur **Linux natif** | CI `install` run [42675213985](https://github.com/Rwanbt/Ravum/actions/runs/42675213985) : préfixe temporaire, manifeste, `--version` |
| [ ] | Installation testée sur **macOS natif** | `NOT_RUN` |
| [ ] | Préfixe par défaut testé (hors temporaire), sans effet de bord | `NOT_RUN` |

## 5. SBOM

| Case | Condition | Preuve attendue |
|---|---|---|
| [x] | `sbom.ps1` génère un SPDX 2.3 JSON valide | `spdxVersion=SPDX-2.3`, packages=63, relations=165 |
| [x] | `sbom.sh` produit un fichier **identique** à `sbom.ps1` | SHA-256 `68826ed4…dc7c` dans les deux cas |
| [x] | Déterminisme vérifié (deux exécutions, horodatage fixe) | hashes identiques, bytes identiques |
| [x] | `SOURCE_DATE_EPOCH=0` neutralise `created` | `1970-01-01T00:00:00Z` |
| [ ] | SBOM archivé avec l'artefact publié (horodatage fixe) | fichier publié + hash |
| [ ] | SBOM signé / attesté | SBOM généré (non signé) ; signature possible mais **clé absente** (D9) |

## 6. Provenance

| Case | Condition | Preuve attendue |
|---|---|---|
| [x] | Manifeste d'installation contient version, commit, date UTC, SHA-256, rustc | `ravum-install.json` observé |
| [x] | `git_dirty` enregistré et signalé à l'utilisateur | `git_dirty: true` + avertissement stderr |
| [x] | `PROVENANCE.md` distingue prouvé / non prouvé | document écrit |
| [ ] | Provenance **signée** (SLSA/attestation) | aucune attestation SLSA ; le mécanisme de signature existe, la clé manque (D9) |
| [ ] | Vérification indépendante du binaire publié | aucun binaire publié ; vérification locale possible par hash |

## 7. Licence

| Case | Condition | Preuve attendue |
|---|---|---|
| [x] | Décision de licence prise (humaine) | `LICENSE-DECISION.md` statut `DECIDED` — `MIT OR Apache-2.0` |
| [x] | Fichiers `LICENSE-MIT` et `LICENSE-APACHE` à la racine | présents (repris de `dev`, commit `fbaa506`, PR #103) |
| [x] | Champ `license` renseigné dans `Cargo.toml` (workspace + crates) | `cargo metadata --locked` → `MIT OR Apache-2.0` pour les **23** membres |
| [x] | SBOM régénéré : `licenseDeclared` cesse d'être `NOASSERTION` | `scripts/sbom.ps1` → 23/23 membres `MIT OR Apache-2.0` |
| [ ] | Notices tierces générées depuis le SBOM | `THIRD-PARTY-NOTICES` — `NOT_DONE` |
| [ ] | `scripts/licence-audit.ps1` exécuté et résultat rapporté | sortie JSON — `NOT_RUN` cette session |

## 8. Documentation

| Case | Condition | Preuve attendue |
|---|---|---|
| [x] | `INSTALL.md` (prérequis, install/désinstall, vérification, dépannage) | document écrit |
| [x] | `SBOM.md` (génération, déterminisme, limites) | document écrit |
| [x] | `PROVENANCE.md` (prouvé/non prouvé) | document écrit |
| [x] | `RELEASE-CHECKLIST.md` (ce fichier) | document écrit |
| [x] | `LICENSE-DECISION.md` (statut `DECIDED`, `MIT OR Apache-2.0`) | document écrit |
| [ ] | Docs relues par le responsable | `PENDING_HUMAN` |

## 9. Rollback

| Case | Condition | Preuve attendue |
|---|---|---|
| [ ] | Procédure de rollback documentée et testée (réinstallation version précédente) | `NOT_RUN` |
| [ ] | Désinstallation vérifiée sur poste propre | partiel (poste de développement uniquement) |
| [ ] | Aucun effet résiduel (registre, service, PATH système) | `NOT_APPLICABLE` (aucun) |

## 10. Publication

| Case | Condition | Preuve attendue |
|---|---|---|
| [ ] | Artefacts publiés (binaire + SBOM + checksums) | `NOT_RUN` |
| [ ] | Notes de version publiées | `NOT_RUN` |
| [ ] | Annonce/rollback plan communiqués | `NOT_RUN` |

## 11. Synthèse honnête au 2026-11-09

- **Vérifié localement (Windows, Git Bash)** : installateurs, manifeste, hash, PATH,
  désinstallation, SBOM déterministe, fail-closed sur build cassé.
- **Non vérifié** : arbre propre, macOS natif, reproductibilité inter-machines, signatures
  de release (mécanisme qualifié, clé absente), publication, rollback. La licence est
  **décidée** (`MIT OR Apache-2.0`, 23/23) et l'installation Linux est **vérifiée en CI**.
- **Conclusion** : la chaîne d'installation et de SBOM est **utilisable en développement**,
  pas qualifiée pour une release publique. Aucune case non cochée ne doit être présentée
  comme faite.
