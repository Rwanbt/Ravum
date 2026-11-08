# MVP-0c — Audit structuré des PR #78 et #80 (lot L0)

> **Nature de ce document.** Audit **read-only** demandé avant toute décision d'intégration.
> Aucune fusion, aucun `git checkout/commit/push/reset`, aucun `git fetch` destructif, aucune
> branche changée, aucun fichier déplacé ou supprimé, aucun code du dépôt exécuté.
> Les seuls fichiers écrits par l'auditeur sont ce rapport et `evidence/mvp0c-pr-audit.json`
> (inventaire exhaustif par fichier, 186 + 304 entrées).
>
> | Champ | Valeur |
> |---|---|
> | Auditeur | Worker A (`git-txn`), mission MVP-0c / lot L0 |
> | Fenêtre d'audit | 2026-11-09 00:44 → 00:57 (Europe/Paris) |
> | Dépôt | `Rwanbt/Ravum` (GitHub), clone local `D:\App\Ravum` |
> | Branche de travail | `feat/mvp0c-hardening` (non modifiée par l'audit) |
> | HEAD local (contextuel) | `19ce0ab` → `95069fd` pendant l'audit, `54177dd` observé juste après : branche d'un **autre worker MVP-0c**, modifiée en continu ; **aucun effet sur les PR #78/#80** (têtes et bases inchangées) |
> | PR auditées | #78 (base `dev`) et #80 (base `feat/3-mvp0a-continuous-loop`, empilée sur #78) |
> | Données brutes | `evidence/mvp0c-pr-audit.json` (JSON validé, 151 562 octets) |

## Résumé exécutif

- **#78 est techniquement fusionnable** : `MERGEABLE`/`CLEAN`, tête `6b37ad3` couverte par des
  checks verts, base `dev` strictement ancêtre de la tête (`dev` = `7841102`, docs-only + 1
  workflow documentaire), 186 fichiers `+25528/−6`, **aucun conflit** simulé, **aucun secret**
  détecté, aucune dépendance externe ajoutée hors lockfile naissant.
- **#80 est techniquement fusionnable après #78**, mais **son `baseRefName` pointe encore sur la
  branche de #78** (`feat/3-mvp0a-continuous-loop` = tête de #78). Le **retarget vers `dev` est
  obligatoire** avant fusion. Ensuite : 304 fichiers `+128220/−304`, 54 commits, checks verts sur
  `ea7200c`, **0 conflit** simulé, **aucun secret**, **0 nouvelle dépendance externe**.
- L'arbre final attendu après `#78 → retarget #80 → #80` est **exactement** l'arbre de `ea7200c`
  (`030e6b0e…`) ; le dépôt passe de 78 à 530 fichiers suivis.
- **Aucun risque bloquant technique** n'a été identifié. Restent des décisions/opérations humaines
  non bloquantes (retarget, stratégie de fusion, contenu d'outillage IA tiers, chemins absolus
  dans les preuves, épinglage des actions CI) : voir §8.

---

## 1. État GitHub vérifié à l'instant T

Commandes utilisées (toutes en lecture seule) :

```bash
gh auth status
gh pr view 78 --json number,state,isDraft,mergeable,mergeStateStatus,headRefName,baseRefName,headRefOid,baseRefOid,additions,deletions,changedFiles,author,url
gh pr view 80 --json number,state,isDraft,mergeable,mergeStateStatus,headRefName,baseRefName,headRefOid,baseRefOid,additions,deletions,changedFiles,author,url
gh api repos/Rwanbt/Ravum/pulls/78 --jq .commits   # idem 80
gh api repos/Rwanbt/Ravum/pulls/78/files --paginate --jq '.[].filename'
gh pr checks 78                                     # idem 80
gh pr list --state open --json number,mergeable,baseRefName,headRefName
gh api repos/Rwanbt/Ravum/branches/dev/protection  # idem main
gh api repos/Rwanbt/Ravum --jq '{allow_merge_commit,allow_squash_merge,allow_rebase_merge,delete_branch_on_merge,default_branch}'
gh api repos/Rwanbt/Ravum/actions/runs/<id>
```

`gh` est authentifié comme `Rwanbt` (scopes `gist`, `read:org`, `repo`, `workflow`). **Aucune
commande git de réseau ou mutante n'a été lancée.**

### 1.1 Pull requests

| | **#78** | **#80** |
|---|---|---|
| Titre | feat(mvp): boucle continue MVP-0a, hot patch verifie et prototype COLLAB-1 | feat(mvp0b): temps reel DSP, Git transactionnel, daemon incremental et collaboration verifiee |
| État | OPEN, non-draft | OPEN, non-draft |
| Mergeable / mergeState | **MERGEABLE / CLEAN** | **MERGEABLE / CLEAN** |
| Tête | `feat/3-mvp0a-continuous-loop` @ `6b37ad3f22bce0ef0f30735ba0a4e32b6a31eda8` | `feat/mvp0b-hardening` @ `ea7200c47991bc17044cf3cb50cbdd78c20005f9` |
| Base | `dev` @ `78411024e2665dafce31b029786ef56d400bdab2` | `feat/3-mvp0a-continuous-loop` @ `6b37ad3f22bce0ef0f30735ba0a4e32b6a31eda8` ⚠️ branche de #78 |
| Fichiers | **186** | **304** |
| Lignes | **+25 528 / −6** | **+128 220 / −304** |
| Commits | **22** | **54** |
| Auteur / dates | Rwanbt, créée 2026-11-08T01:19:42Z, maj 2026-11-08T02:24:24Z | Rwanbt, créée 2026-11-08T14:07:44Z, maj 2026-11-08T22:24:05Z |
| URL | https://github.com/Rwanbt/Ravum/pull/78 | https://github.com/Rwanbt/Ravum/pull/80 |

**Recoupement local ↔ GitHub.** `gh pr diff <n> --name-status` **n'existe pas** dans `gh` 2.x
(options réelles : `--name-only`, `--patch`) : la commande du brief est donc inopérante. La
vérification a été faite via `gh api …/pulls/<n>/files --paginate` puis recoupée avec
`git diff --name-status <base> <head>` :
- #78 : 186 noms API = 186 noms locaux (0 écart) ; statuts API `added=183 modified=3` = locaux.
- #80 : 304 noms API = 304 noms locaux (0 écart) ; statuts API `added=269 modified=35` = locaux.
- Aucune PR ouverte en dehors de #78 et #80.

### 1.2 Chaîne d'ascendance et refs locales

```text
git rev-parse origin/dev                               → 78411024e2665dafce31b029786ef56d400bdab2
git rev-parse origin/main                              → 78411024…  (dev == main)
git rev-parse origin/feat/3-mvp0a-continuous-loop      → 6b37ad3f22bce0ef0f30735ba0a4e32b6a31eda8
git rev-parse origin/feat/mvp0b-hardening              → ea7200c47991bc17044cf3cb50cbdd78c20005f9
git merge-base --is-ancestor origin/dev origin/feat/3-mvp0a-continuous-loop        → exit 0 (vrai)
git merge-base --is-ancestor origin/feat/3-mvp0a-continuous-loop origin/feat/mvp0b-hardening → exit 0 (vrai)
git rev-list --count origin/dev..origin/feat/3-mvp0a-continuous-loop               → 22
git rev-list --count origin/dev..origin/feat/mvp0b-hardening                       → 76
git rev-list --count origin/feat/3-mvp0a-continuous-loop..origin/feat/mvp0b-hardening → 54
```

Conséquence : `#80` **contient déjà** la tête de `#78` ; après fusion de #78, la base de fusion de
#80 restera `6b37ad3` et la fusion sera propre **aux arbres actuels** (§7).

### 1.3 CI observée

| Run | Nom | Conclusion | Tête | Lien |
|---|---|---|---|---|
| 42529748229 | MVP product checks (build, tests, demos) | **success** | `ea7200c` | https://github.com/Rwanbt/Ravum/actions/runs/42529748229 |
| 42529748447 | MVP impact-selected checks (conservative, fail-closed) | **success** | `ea7200c` | https://github.com/Rwanbt/Ravum/actions/runs/42529748447 |
| 42438404239 | MVP product checks (build, tests, demos) | **success** | `6b37ad3` | https://github.com/Rwanbt/Ravum/actions/runs/42438404239 |
| 42438407236 | Documentation integrity | **success** | `6b37ad3` | https://github.com/Rwanbt/Ravum/actions/runs/42438407236 |
| 42438407246 | MVP product checks (build, tests, demos) | **success** | `6b37ad3` | https://github.com/Rwanbt/Ravum/actions/runs/42438407246 |

Checks de PR (`gh pr checks`) : **#78** — `docs` pass 4 s, `mvp` pass 1 m 23 s (×2 runs sur la même
tête) ; **#80** — `Bounded automatic GC…` pass 55 s, `impact` pass 2 m 28 s, `mvp` pass 3 m 3 s.
Aucun check en échec ou en attente au moment de l'audit.

### 1.4 Protection de branches et réglages de fusion

- `dev` : revue de PR activée avec **0 approbation requise**, `dismiss_stale_reviews=true`,
  `required_conversation_resolution=true`, force-push et suppression interdits ;
  **aucun required status check** déclaré sur `dev` dans la réponse API.
- `main` : required status check **`mvp`** (`strict=false`), revue 0 approbation, mêmes protections.
- Réglages dépôt : `allow_merge_commit=true`, `allow_squash_merge=true`, `allow_rebase_merge=true`,
  `delete_branch_on_merge=false`, branche par défaut `main`.

---

## 2. Classification exhaustive des fichiers

**Méthode.** Chaque fichier du diff est classé par règle de chemin (liste ci-dessous), avec
**taille du blob** à la tête de PR pour les ajouts/modifications (aucune suppression de fichier
dans les deux PR) et lignes issues de `git diff --numstat --no-renames`. Commandes :

```bash
git diff --name-status  <base> <head>
git diff --numstat --no-renames <base> <head>
git diff --name-status -M <base> <head>          # contrôle des renommages → 0
git ls-tree -r -l <head>                          # tailles des blobs
```

Règles (première qui s'applique) : `fixtures?/` → **Fixtures** ; `evidence/` → **Preuves** ;
`.claude/`, `.agents/`, `.ai-native/`, `tools/ai_docs/` → **Outillage IA** ; `docs/` ou `*.md` →
**Documentation** ; `.github/workflows/` → **CI/workflows** ; `.github/` → **Config GitHub** ;
`benchmarks/` → **Benchmarks/harness S8** ; `scripts/` → **Scripts** ; `tools/` → **Outils** ;
`crates/*/tests/` → **Tests Rust** ; `crates/*/benches/` → **Benchmarks Rust** ;
`crates/*/src/**.rs` → **Code produit Rust** ; `Cargo.toml` racine/crates → **Manifests Cargo** ;
`Cargo.lock` → **Lockfile (généré)** ; `apps/` → **Application démo** ; `target/` → **Générés** ;
extensions binaires → **Binaires/médias** ; `*.json|jsonl|txt` → **Résultats/données** ;
`*.ps1|mjs|js|css|html|sql|yml|toml|.gitignore|.gitattributes` → **Config/scripts divers**.

> L'inventaire **fichier par fichier** (statut, catégorie, taille, ajouts/suppressions) des deux PR
> est dans `evidence/mvp0c-pr-audit.json`, clé `files."78"` / `files."80"`.
> Exemple : `jq '[.files."80"[] | select(.category=="Preuves (evidence)")] | length' evidence/mvp0c-pr-audit.json`

### 2.1 PR #78 — `dev` (`7841102`) → `6b37ad3` : 186 fichiers, **+25 528 / −6**, 995 058 octets

| Catégorie | Fichiers | Octets (blobs tête) | Lignes | Statuts |
|---|---:|---:|---:|---|
| Outillage IA (prompts/skills/config) | 47 | 337 775 | +9 092 / −0 | A47 |
| Code produit Rust (crates) | 42 | 291 967 | +8 440 / −0 | A42 |
| Documentation | 14 | 173 333 | +2 491 / −6 | A12, M2 |
| Tests Rust | 8 | 51 294 | +1 363 / −0 | A8 |
| Application démo | 10 | 43 820 | +1 322 / −0 | A10 |
| Preuves (evidence) | 24 | 37 765 | +920 / −0 | A24 |
| Scripts / outillage | 5 | 21 490 | +523 / −0 | A5 |
| Fixtures | 19 | 14 654 | +535 / −0 | A19 |
| Lockfile (généré) | 1 | 11 174 | +481 / −0 | A1 |
| Manifests Cargo (workspace) | 12 | 6 128 | +208 / −0 | A12 |
| CI / workflows | 1 | 3 416 | +89 / −0 | A1 |
| Résultats / données (`conventions.json`) | 1 | 1 244 | +41 / −0 | A1 |
| Config / scripts divers (`.gitattributes`, `.gitignore`) | 2 | 998 | +23 / −0 | A1, M1 |
| **Total** | **186** | **995 058** | **+25 528 / −6** | **A183, M3** |

Aucun binaire, aucun renommage, aucun fichier supprimé. #78 introduit **tout le workspace Rust**
depuis une base `dev` docs-only : c'est ce qui explique la part « code produit » (42 fichiers,
292 Ko) et « manifests » (14 membres + racine, dont 2 manifests d'apps classés avec la démo).

### 2.2 PR #80 — `6b37ad3` → `ea7200c` : 304 fichiers, **+128 220 / −304**, 4 826 012 octets

| Catégorie | Fichiers | Octets (blobs tête) | Lignes | Statuts |
|---|---:|---:|---:|---|
| Preuves (evidence) | 79 | 2 371 374 | +74 197 / −1 | A78, M1 |
| Code produit Rust (crates) | 58 | 1 299 647 | +30 006 / −235 | A38, M20 |
| Documentation | 21 | 456 953 | +6 899 / −27 | A18, M3 |
| Tests Rust | 13 | 206 508 | +5 599 / −0 | A13 |
| Application démo (UI + serveur + harness E2E) | 9 | 119 053 | +2 555 / −36 | A5, M4 |
| Benchmarks / harness S8 | 9 | 106 592 | +2 839 / −0 | A9 |
| Scripts / outillage | 11 | 103 205 | +2 091 / −1 | A10, M1 |
| Outils build/impact (`tools/impact/`) | 2 | 92 341 | +2 208 / −0 | A2 |
| CI / workflows | 2 | 28 208 | +476 / −0 | A1, M1 |
| Fixtures | 94 | 26 714 | +1 298 / −0 | A94 |
| Lockfile (généré) | 1 | 11 335 | +12 / −0 | M1 |
| Manifests Cargo (workspace) | 5 | 4 082 | +40 / −4 | A1, M4 |
| **Total** | **304** | **4 826 012** | **+128 220 / −304** | **A269, M35** |

Notes de lecture :
- **49 % des octets** ajoutés par #80 sont des **preuves JSON** (2,37 Mo ; 4 fichiers > 100 Ko,
  max 255 092 B) ; les plus gros : `mvp0b-s8-B` (255 Ko), `mvp0b-s8-A` (205 Ko),
  `mvp0b-s8-C` (173 Ko), `mvp0b-incremental-bench` (148 Ko).
- Aucun binaire, aucun renommage, aucun fichier supprimé.
- Les manifests d'apps/fixtures sont comptés dans leurs catégories (`Application démo`,
  `Fixtures`, `Benchmarks`) ; « Manifests Cargo » ne recense que racine + crates du workspace.
- Le plus gros fichier **code** modifié/ajouté : `crates/ravum-dsp-harness/src/bin/ravum-audio-probe.rs`
  (262 857 B) — outil de mesure audio (binaire de probe), pas du code produit runtime.

---

## 3. Fichiers volumineux, LFS et artefacts

### 3.1 Seuils demandés

| Seuil | Fichiers changés par #78 | Fichiers changés par #80 | Arbre complet à `ea7200c` |
|---|---|---|---|
| > 500 Ko (512 000 B) | **aucun** | **aucun** | **aucun** |
| > 1 Mo (1 048 576 B) | **aucun** | **aucun** | **aucun** |

Top 15 des fichiers changés (toutes PR confondues) :

| Octets | PR | Catégorie | Fichier |
|---:|---|---|---|
| 262 857 | #80 | Code produit Rust | `crates/ravum-dsp-harness/src/bin/ravum-audio-probe.rs` |
| 255 092 | #80 | Preuves | `evidence/mvp0b-s8-B-2026-11-08.json` |
| 204 809 | #80 | Preuves | `evidence/mvp0b-s8-A-2026-11-08.json` |
| 172 898 | #80 | Preuves | `evidence/mvp0b-s8-C-2026-11-08.json` |
| 147 594 | #80 | Preuves | `evidence/mvp0b-incremental-bench.json` |
| 79 650 | #78 | Documentation | `docs/rfc/RFC-SHARED-WORKSPACE-MULTIAGENT.md` |
| 78 204 | #80 | Code produit Rust | `crates/ravum-daemon/src/gc.rs` |
| 75 434 | #80 | Code produit Rust | `crates/ravum-workspace-coordinator/src/git_writer.rs` |
| 74 108 | #80 | Code produit Rust | `crates/ravum-cli/src/collab_cycle.rs` |
| 71 543 | #80 | Preuves | `evidence/mvp0b-daemon-gc-2026-11-08.json` |
| 69 473 | #80 | Preuves | `evidence/mvp0b-daemon-soak-multi-2026-11-08.json` |
| 63 320 | #80 | Documentation | `docs/mvp/MVP-0b-audio-host.md` |
| 58 699 | #80 | Documentation | `docs/mvp/MVP-0b-REPORT.md` |
| 55 763 | #80 | Documentation | `docs/mvp/MVP-0b-incremental.md` |
| 52 009 | #80 | Outils | `tools/impact/impact.mjs` |

Le plus gros fichier **de l'arbre complet** est préexistant sur `dev` :
`docs/reviews/2026-11-08-consolidation.md` (496 653 B < 500 Ko). Aucun LFS nécessaire.

### 3.2 LFS / normalisation

`#78` ajoute `.gitattributes` (518 B) mais il ne contient **que** des règles de fin de ligne et
`binary` (`* text=auto eol=lf`, `*.ps1/*.psm1/*.bat/*.cmd eol=crlf`, `*.png/binary`, …) — **aucune
règle `filter=lfs`**. Vérifications : `git grep -l -I 'git-lfs' 6b37ad3` et `ea7200c` → 0 résultat ;
`git check-attr filter diff merge text` sur les plus gros fichiers → `filter: unspecified`.
**Conclusion** : pas de fichier LFS, pas de pointeur brisé ; Git LFS n'est pas requis pour cette
intégration. Si les preuves grossissent (> 1 Mo/fichier ou > 10 Mo cumulés), la décision
humaine pourrait être des **artefacts CI** (`actions/upload-artifact`, déjà utilisé pour les
captures browser E2E et l'evidence auto-GC) plutôt que LFS — mais **la règle actuelle de preuve
versionnée dans le dépôt reste respectée (aucun fichier > 500 Ko)** et rien ne doit être déplacé
sans décision humaine.

---

## 4. Recherche de risques

### 4.1 Secrets, jetons, chemins personnels

Lignes ajoutées analysées : **25 528 (#78)** et **128 181 (#80)**. Motifs recherchés :
`ghp_/gho_/ghs_/ghu_/ghr_`, `github_pat_`, `sk-`/`sk-ant-`, `AKIA…`, `xox[baprs]-`, `AIza…`,
bloc de clé privée, JWT `eyJ…`, affectations `secret|password|token|api_key`, `Bearer` littéral,
`${{ secrets.* }}` dans les workflows.

**Résultat : 0 secret, 0 token, 0 clé détectés dans les deux PR.** Les occurrences des mots
`token` sont des identifiants de test E2E (`token` du harness CDP), des `CancelToken`/tokeniseurs
(en Rust) ou du vocabulaire de rapport — aucune valeur secrète. Aucun fichier `.env`, `.pem`,
`id_rsa`, `.key` n'est ajouté (les 4 « hits » de nommage étaient des identifiants Rust
`CacheLookupKey…`).

**Chemins personnels (divulgation d'environnement, pas un secret)** :

| PR | Motif | Occurrences | Fichiers |
|---|---|---:|---|
| #78 | `<lecteur>:\App\<projet>` | 4 | 3 (`.claude/settings.json`, `tools/ai_docs/config.sh.example`, `evidence/multi-agent-live/README.md`) |
| #80 | `C:\Users\<utilisateur>` (nom d'utilisateur réel) | 124 | 4 preuves S8 (`mvp0b-s8-{A,B,C,summary}-2026-11-08.json`) |
| #80 | `<lecteur>:\App\<projet>` | 11 | 10 (preuves + docs) |

`D:\App\Seno` apparaît **2 fois** dans `docs/mvp/MVP-0b-blockers.md`, uniquement pour déclarer
« projet distinct, non modifié » (pas une fuite de secret). **Recommandation non bloquante** :
normaliser/redacter ces chemins machine si les preuves JSON sont republiées hors du dépôt ; le
nom d'utilisateur Windows y est déjà exposé par l'historique Git public, donc pas d'urgence.

### 4.2 Workflows CI ajoutés/modifiés

| Fichier | Changement | Taille | Déclencheurs | Analyse |
|---|---|---:|---|---|
| `.github/workflows/docs-check.yml` | **inchangé** (présent sur `dev`) | 570 B | push/PR `main,dev` | Ubuntu, Python 3.12, `tools/validate_repo_docs.py`, `permissions: contents: read` |
| `.github/workflows/mvp.yml` | **#78 : A** (+89 l.) puis **#80 : M** (+179 l.) | 3 416 → 13 428 B | push `main,dev,feat/**,fix/**`, PR `main,dev`, dispatch | `windows-latest`, `permissions: contents: read`, jobs `mvp` + `auto-gc` ; build workspace, `cargo test --workspace -- --test-threads=1`, démos M1/COLLAB-1/M2/M3/M4, browser E2E CDP, bench informatif, soak auto-GC borné ; artefacts `browser-e2e-evidence`, `auto-gc-evidence` |
| `.github/workflows/mvp-impact.yml` | **#80 : A** (+297 l.) | 14 780 B | push `feat/**,fix/**`, PR `main,dev`, dispatch | `windows-latest`, `fetch-depth: 0`, `git fetch origin dev main`, exécute `node tools/impact/impact.mjs`, puis suite complète (FULL légitime) ou suites sélectives ; mapping incertain ⇒ **échec explicite** ; `permissions: contents: read` |

Constats de sécurité CI :
- `permissions: contents: read` sur tous les jobs ; **aucun `${{ secrets.* }}`** dans les workflows ;
  aucun `pull_request_target` ; pas d'écriture de ref/PR depuis la CI.
- Actions épinglées par **tag majeur** (`actions/checkout@v4`, `actions/cache@v4`,
  `actions/upload-artifact@v4`, `actions/setup-node@v4`, `actions/setup-python@v5`,
  `dtolnay/rust-toolchain@stable`), **pas par SHA**. Risque supply-chain non bloquant ;
  recommandation d'épingler par SHA si durcissement souhaité.
- Les workflows exécutent des scripts du dépôt (`scripts/demo-*.ps1`, `scripts/impact.ps1`) :
  c'est le comportement attendu d'une CI de ce projet, mais cela signifie que **le code PR est
  exécuté par la CI** (déjà le cas, checks verts) ; `-ExecutionPolicy Bypass` est passé au
  processus PowerShell (pas de politique machine). `mvp.yml` réalise un vrai commit **dans un
  dépôt jetable** créé par `scripts/demo-collab.ps1` sous `target/`, pas dans le checkout CI.
- Coût/portée : jobs `windows-latest` (mvp ≈ 3 min, impact ≈ 2,5 min) ; `mvp-impact.yml` tourne
  aussi sur les push de branches `feat/**` (auto-contrôle documenté par les auteurs).

### 4.3 Dépendances (Cargo.lock / crates.io)

- `Cargo.lock` : **481 lignes ajoutées par #78** (nouveau workspace), **+12 lignes par #80** ;
  format de lockfile `3`. **55 paquets** au total, dont **41 tiers avec source** et **14 membres du
  workspace sans source** ; **1 seule source distincte** : `registry+https://github.com/rust-lang/crates.io-index` ;
  **0 dépendance git, 0 dépendance `path` externe** (les `path` internes au workspace n'apparaissent
  pas avec `source` et correspondent aux 14 membres).
- **#80 n'ajoute aucune dépendance externe** : uniquement du câblage interne — membre
  `crates/ravum-daemon` (serde/serde_json/sha2 déjà présents), `ravum-cli` +`ravum-daemon` et
  `ravum-workspace-coordinator` promu de dev-dependency à dependency d'exécution pour
  `collab-cycle`, `ravum-workspace-coordinator` +`ravum-rust-adapter` (commenté « bins seulement »),
  `ravum-dsp-harness` +`serde_json`, `ravum-demo-host` +`serde_json` (manifeste d'app).
  Racine `Cargo.toml` : +1 membre, +1 dependency workspace.
- **Limite** : les licences des 41 paquets tiers ne sont **pas re-vérifiées** ici (audit read-only,
  pas de `cargo metadata`/réseau) ; la claim antérieure « 41 dépendances toutes permissives
  (MIT/Apache-2.0/ISC/Unlicense/Zlib/Unicode-3.0) » provient des rapports MVP-0b et n'est **pas
  re-mesurée** par cet audit.

### 4.4 Licences et provenance du contenu tiers

- **Aucun fichier `LICENSE`/`COPYING` ajouté** par #78 ni #80 (vérifié `git diff --name-status` et
  racine `ea7200c`) : la décision de licence reste **réservée au mainteneur (issue #34)**, ce qui
  est cohérent avec `scripts/licence-audit.ps1` (audit statique local, non exécuté ici).
- **Contenu tiers embarqué** : `.claude/skills/` et `.agents/skills/` recopient notamment des skills
  de la suite tierce « AI-Native Dev Stack » / **gstack** (`stack-upgrade` cite `gstack`,
  `install.sh`, `UPDATING.md`, `PORTABILITY.md`, `scripts/stack-update-check.sh`,
  `scripts/stack-upgrade.sh` — **fichiers absents du dépôt**), et `conventions.json` référence
  `scripts/validate_conventions.py`, **absent lui aussi**. Aucune notice de licence tierce n'est
  fournie dans le dépôt. Risque **non bloquant** mais réel : à trancher humainement
  (attribution/licence des skills, ou exclusion de l'outillage IA du périmètre du dépôt).

### 4.5 Outillage IA commité (surface de gouvernance)

- #78 ajoute **47 fichiers / 337 775 B** d'outillage IA : `.claude/skills/*` (13 fichiers),
  `.agents/skills/*` (**13 copies blob-identiques** de `.claude` : 105 111 octets dupliqués),
  `.ai-native/{trust,lifecycle,templates,docs}`, `.claude/settings.json`, `tools/ai_docs/*`.
- `.claude/settings.json` (302 B) commité active un hook agent `PostToolUse` :
  `powershell -NoProfile -ExecutionPolicy Bypass -File "D:/App/Ravum/tools/ai_docs/run_hook.ps1"`
  — chemin **absolu de la machine de l'auteur**, exécuté à chaque `Edit|Write` pour cet agent.
  Sans effet sur le produit ni sur la CI, mais : non portable, invisible pour les autres
  utilisateurs, et surface d'exécution locale. **Décision humaine souhaitable** (garder, paramétrer,
  ou ne pas versionner la config locale).
- `.ai-native/trust/*.json` : traces d'approbation du propriétaire (`promotion_policy=explicit`,
  `recorded_owner_ack`), **aucune allowlist d'auto-approbation** observée. `.ai-native/lifecycle/
  state.json` (15 486 B) et `…/transactions/*.json` (27 821 B) sont de l'état de session d'outil
  IA versionné (traçabilité, pas de secret).
- Vérification réseau : `tools/ai_docs/*` n'utilisent que `subprocess` local (python,
  `graphify` si présent), **aucun appel HTTP** ; `run_hook.ps1` écrit un JSON temporaire et
  délègue à `update_on_edit.py`, sans exfiltration.

### 4.6 Scripts, hooks et opérations destructives

- Inventaire : #78 ajoute 5 scripts `scripts/*.ps1` (21 490 B) et 11 outils `tools/ai_docs/*` ;
  #80 ajoute 11 scripts (103 205 B), `tools/impact/*` (2 fichiers, 92 341 B) et le harness
  `benchmarks/s8/` (9 fichiers, 106 592 B) + harness E2E `apps/ravum-todo/e2e/` (5 `.mjs`,
  96 540 B, sans npm : aucun `package.json`/`package-lock.json` dans le dépôt).
- `Remove-Item -Recurse` : présent dans plusieurs démos/soaks, mais **uniquement sur des
  répertoires de travail locaux** (`$work` = `target\ravum\…`, `WorkDir` de soak, `target/build-flags-exp`,
  artefact dir) — jamais sur le dépôt ni sur des chemins utilisateur. `scripts/demo-collab.ps1`
  crée **son propre dépôt Git jetable** sous `target\ravum\collab-demo` (commentaire explicite et
  code vérifiés) ; `benchmarks/s8` crée ses propres dépôts/worktrees jetables et y configure
  `core.hooksPath` pour **son** dépôt de fixture.
- Aucun `git reset --hard`, `git clean -f`, `git push --force`, `--force` destructif ou
  `Set-ExecutionPolicy MachinePolicy/LocalMachine` sur le dépôt principal. `reg` n'apparaît que
  pour une **lecture** `HKLM:\HARDWARE\…\ProcessorNameString` (nom du CPU) dans
  `scripts/build-flags.ps1`.
- `--no-verify` : une seule occurrence dans #80, **dans un commentaire d'en-tête** de
  `git_writer.rs` décrivant l'ancien prototype abandonné — **aucune invocation**. Aucun hook Git
  ajouté au dépôt (pas de `.githooks`, pas de `core.hooksPath` versionné) ; le seul hook est le
  hook d'agent Claude décrit en §4.5.

### 4.7 Tests et non-régression (contrôle rapide, secondaire)

Sur les diffs : **0 fonction `#[test]` supprimée**, **0 `#[ignore]` ajouté**, **0 `todo!`/
`unimplemented!` ajouté**, **0 fichier de test supprimé**. #80 supprime **2 lignes `assert`** dans
`crates/ravum-dsp-harness/src/alloc.rs`, **remplacées par un test plus strict** dans le même hunk
(comptage des désallocations ajouté, reset explicite du compteur par thread, nouvelle assertion
`deallocations >= 1`, nouveau test `resetting_clears_both_counters`) : couverture renforcée, pas
affaiblie. Ce contrôle ne remplace **pas** une exécution des tests (non faite ici, read-only).

### 4.8 Contenus générés / volumineux dans le dépôt

- `Cargo.lock` (généré) : 1 fichier dans chaque PR — attendu pour un workspace Rust ; à conserver.
- `evidence/` : 24 fichiers / 37 765 B (#78) et 79 fichiers / 2 371 374 B (#80) ; 4 fichiers
  > 100 Ko mais **aucun > 500 Ko**. `benchmarks/s8/results/latest.md` est un résultat de campagne
  versionné. Rien n'est binaire.
- `.gitignore` (#78) ignore `evidence/local/`, `tools/ai_docs/config.sh`, `.ai-native/state/` :
  cohérent (config locale et preuves locales non versionnées).

---

## 5. Base `dev`, conséquence pour la fusion, séquence proposée (sans exécution)

### 5.1 Vérification de la base (avec correction du constat initial)

```text
git rev-parse origin/dev        → 78411024e2665dafce31b029786ef56d400bdab2
git rev-parse origin/main       → 78411024…  (identique)
git rev-list --count origin/dev → 78 commits suivis (arbre : 78 fichiers)
git ls-tree -r --name-only origin/dev | grep -c '^crates/'  → 0
git ls-tree -r --name-only origin/dev | grep '^\.github/'   → 7 fichiers
```

Contenu `.github` de `dev` : 5 modèles d'issue + `pull_request_template.md` +
**`.github/workflows/docs-check.yml` (570 B)**.

> ⚠️ **Correction factuelle** : le constat « `dev` ne contient ni crates ni workflows » est
> **inexact pour les workflows** — `dev` contient bien `docs-check.yml` (CI documentaire, non
> produit) et 7 fichiers `.github`. En revanche `dev` **ne contient effectivement aucun crate
> Rust** (0 fichier `crates/`, pas de `Cargo.toml`/`Cargo.lock`).

**Conséquence** : #78 introduit le workspace Rust complet par-dessus une base documentation ;
c'est un ajout massif mais **strictement additif** (183 ajouts / 3 modifications, aucune
suppression). Après fusion, `dev` devient la branche d'intégration du produit ; `docs-check.yml`
continue de tourner, `mvp.yml` (issu de #78) s'ajoutera aux push sur `dev`.

### 5.2 Séquence de fusion proposée — **à exécuter par l'humain, non exécutée ici**

```bash
# 0) Pré-vol (lecture seule) — vérifier que les têtes n'ont pas bougé
gh pr view 78 --json state,mergeable,mergeStateStatus,headRefOid,baseRefOid
gh pr view 80 --json state,mergeable,mergeStateStatus,headRefOid,baseRefOid
gh pr checks 78
gh pr checks 80
git fetch origin                      # geste humain ; sans --prune, non destructif
git rev-parse origin/dev              # doit rester 7841102… avant fusion #78

# 1) Fusionner #78 dans dev par merge commit (recommandé, voir note ci-dessous)
gh pr merge 78 --merge --match-head-commit 6b37ad3f22bce0ef0f30735ba0a4e32b6a31eda8

# 2) Vérifier l'état post-#78
git fetch origin
git rev-parse origin/dev                                  # attendu : nouveau merge commit (M78)
git rev-parse 'origin/dev^{tree}'                         # attendu : 64bd3ad235d2846702ab5487c358df104e391712
git diff --shortstat 7841102..origin/dev                  # attendu : 186 files, +25528/-6

# 3) Retarget #80 AVANT sa fusion (base actuelle = branche de #78)
gh pr edit 80 --base dev
gh pr view 80 --json baseRefName,mergeable,mergeStateStatus

# 4) Fusionner #80
gh pr merge 80 --merge --match-head-commit ea7200c47991bc17044cf3cb50cbdd78c20005f9

# 5) Vérifier l'état final
git fetch origin
git rev-parse 'origin/dev^{tree}'                         # attendu : 030e6b0e9c690d6e88cf865f4ceef9b8bebb5bc8
git rev-parse origin/feat/mvp0b-hardening                 # attendu : ea7200c… (inchangé)
git diff --shortstat <M78>..origin/dev                    # attendu : 304 files, +128220/-304
git diff --shortstat 7841102..origin/dev                  # cumul : 456 files, +153451/-13
git ls-tree -r --name-only origin/dev | wc -l             # attendu : 530
```

Notes d'opération :
- **Pourquoi un merge commit et pas un squash/rebase pour #78** : #80 contient la tête de #78.
  Un merge commit de #78 préserve cette ascendance et rend la fusion de #80 triviale (§7). Un
  squash/rebase de #78 réécrirait la base de #80 et exigerait un rebase + nouvelle validation CI ;
  à éviter sauf décision explicite. Les trois méthodes sont autorisées par les réglages du dépôt.
- **Retarget obligatoire** : `delete_branch_on_merge=false`, donc la branche
  `feat/3-mvp0a-continuous-loop` survivra à la fusion de #78 ; sans `gh pr edit 80 --base dev`,
  #80 resterait ciblée sur une branche déjà fusionnée. Si la branche était supprimée, GitHub
  retargerait #80 vers la branche par défaut (`main`) — à ne pas laisser faire.
- La protection de `dev` **n'exige aucun status check** : l'humain doit s'assurer que les checks
  sont verts (ils le sont sur les têtes actuelles) et que le retarget n'a pas déclenché un état
  `UNKNOWN`/`DIRTY` avant de fusionner #80.

---

## 6. Diff réel attendu après fusion de #78 (commandes exactes, non exécutées)

Valeurs **pré-calculées en lecture seule** (elles serviront de référence après la fusion) :

| Mesure | Commande de référence | Valeur attendue |
|---|---|---|
| Diff de #78 | `git diff --shortstat 7841102..6b37ad3` | **186 files, +25 528 / −6** |
| Diff propre de #80 | `git diff --shortstat 6b37ad3..ea7200c` | **304 files, +128 220 / −304** |
| Cumul #78+#80 | `git diff --shortstat 7841102..ea7200c` | **456 files, +153 451 / −13** |
| Tree après #78 | `git rev-parse '6b37ad3^{tree}'` | `64bd3ad235d2846702ab5487c358df104e391712` (tree de la tête #78) |
| Tree final après #80 | `git rev-parse 'ea7200c^{tree}'` | `030e6b0e9c690d6e88cf865f4ceef9b8bebb5bc8` |
| Fichiers suivis | `git ls-tree -r --name-only <ref> | wc -l` | 78 (`dev`) → **530** (après les deux) |

Recalcul après fusion (mêmes commandes avec les refs réelles) :

```bash
git fetch origin
B78=78411024e2665dafce31b029786ef56d400bdab2      # dev avant
H78=6b37ad3f22bce0ef0f30735ba0a4e32b6a31eda8      # tête #78
H80=ea7200c47991bc17044cf3cb50cbdd78c20005f9      # tête #80
M78=$(git rev-parse origin/dev)                   # après fusion #78
git rev-parse "$M78^{tree}"                        # == git rev-parse "$H78^{tree}"
git diff --shortstat "$B78..$M78"                  # == 186 files, +25528/-6
git rev-parse 'origin/dev^{tree}'                  # après fusion #80 : == tree(H80)
git diff --shortstat "$M78..origin/dev"            # == 304 files, +128220/-304
git diff --shortstat "$B78..origin/dev"            # == 456 files, +153451/-13
git ls-tree -r --name-only origin/dev | wc -l      # == 530
```

---

## 7. Risques de conflit entre #78 et #80

- **34 chemins sont touchés par les deux PR** (liste complète) :
  `.github/workflows/mvp.yml`, `Cargo.lock`, `Cargo.toml`, `README.md`,
  `apps/ravum-todo/src/http.rs`, `apps/ravum-todo/src/routes.rs`,
  `apps/ravum-todo/static/app.js`, `apps/ravum-todo/static/index.html`,
  `crates/ravum-cli/Cargo.toml`, `crates/ravum-cli/src/commands.rs`,
  `crates/ravum-cli/src/dsp.rs`, `crates/ravum-cli/src/main.rs`, `crates/ravum-cli/src/watch.rs`,
  `crates/ravum-coordinator/src/pipeline.rs`, `crates/ravum-dsp-harness/Cargo.toml`,
  `crates/ravum-dsp-harness/src/alloc.rs`, `crates/ravum-dsp-harness/src/evidence.rs`,
  `crates/ravum-dsp-harness/src/lib.rs`, `crates/ravum-dsp-harness/src/measure.rs`,
  `crates/ravum-patch-host/src/block.rs`, `crates/ravum-patch-host/src/lib.rs`,
  `crates/ravum-patch-host/src/reclaimer.rs`, `crates/ravum-patch-host/src/slot.rs`,
  `crates/ravum-proof/src/broker.rs`, `crates/ravum-protocol/src/dsp.rs`,
  `crates/ravum-rust-adapter/src/parse.rs`, `crates/ravum-workspace-coordinator/Cargo.toml`,
  `crates/ravum-workspace-coordinator/src/bin/ravum-collab.rs`,
  `crates/ravum-workspace-coordinator/src/coordinator.rs`,
  `crates/ravum-workspace-coordinator/src/git_writer.rs`,
  `crates/ravum-workspace-coordinator/src/lib.rs`, `docs/architecture/STATUS.md`,
  `evidence/README.md`, `scripts/demo-collab.ps1`.
- **Mais aucun conflit attendu dans l'ordre #78 → #80**, par construction :
  - `dev` est ancêtre de `6b37ad3` (tête #78), elle‑même ancêtre de `ea7200c` (tête #80) ;
  - après fusion de #78, la base de fusion de #80 est `6b37ad3` et le côté `dev` a **exactement
    l'arbre de `6b37ad3`** ; le côté #80 est `6b37ad3 + 54 commits`. La fusion à trois est donc
    une extension linéaire du côté #80.
- **Simulation read-only** (ancien `git merge-tree`, aucun objet écrit) :

  ```bash
  git merge-tree 64bd3ad235d2846702ab5487c358df104e391712 \
                 6b37ad3f22bce0ef0f30735ba0a4e32b6a31eda8 \
                 ea7200c47991bc17044cf3cb50cbdd78c20005f9
  # → 130 674 lignes de sortie informative, 0 marqueur de conflit (<<<<<<< / ======= / >>>>>>>),
  #   0 section "changed in both" ; exit 0.
  ```
- **Risque résiduel** : uniquement si `dev` reçoit d'autres commits avant la fusion de #80
  (aucune autre PR ouverte au moment de l'audit ; les deltas locaux de `feat/mvp0c-hardening` ne
  sont pas des PR). Dans ce cas, recalculer `git merge-base origin/dev ea7200c` et rejouer la
  simulation avant fusion.

---

## 8. Risques bloquants vs non bloquants — verdict technique

### Bloquants (aucun)

Aucun risque bloquant technique n'a été trouvé :
- les deux PR sont `MERGEABLE`/`CLEAN` et leurs têtes sont couvertes par des checks verts ;
- **aucune modification de fichier supprimée** (0 `D`), 0 renommage, 0 binaire ;
- **aucun conflit** dans la séquence proposée (simulation + ascendance vérifiées) ;
- **aucun secret/token/clé** détecté dans les 153 709 lignes ajoutées au total ;
- **aucune dépendance externe ajoutée par #80** ; lockfile cohérent, 100 % crates.io ;
- aucun test supprimé/désactivé, aucune assertion affaiblie (2 remplacées par plus strict).

### Non bloquants mais nécessitant une décision ou une opération humaine

1. **Retarget de #80 vers `dev` obligatoire** avant fusion (base = branche de #78). Opération `gh`,
   à faire après la fusion de #78.
2. **Stratégie de fusion** : merge commit recommandé pour #78 (préserve l'ascendance de #80) ;
   squash/rebase possibles techniquement mais imposent un rebase/validation supplémentaire de #80.
3. **Protection de `dev` sans required status check** : la CI est verte aujourd'hui, mais la
   protection ne l'impose pas ; vérifier les checks après le retarget.
4. **Outillage IA tiers et licence** : 47 fichiers d'outillage IA (dont skills copiés d'une suite
   tierce « AI-Native Dev Stack »/gstack sans notice de licence, références à des fichiers
   absents) et un hook d'agent commité avec chemin absolu `D:/App/Ravum/...`. Décision humaine :
   conserver/paramétrer/externaliser. Non bloquant pour la fusion, mais dette de gouvernance.
5. **Chemins personnels dans les preuves** (`C:\Users\<utilisateur>` ×124, chemins `D:\App\…`) :
   à normaliser si republication hors dépôt ; ce ne sont pas des secrets.
6. **Durcissement supply-chain CI** : actions épinglées par tag majeur, pas par SHA.
7. **Volume des preuves** : 2,37 Mo de JSON pour #80 (aucun fichier > 500 Ko) ; LFS non requis,
   artefacts CI déjà en place pour les preuves non versionnées.

### Verdict

- **#78 : fusionnable techniquement** dans `dev` (après revue humaine). Aucune objection
  technique ; les seules réserves sont de gouvernance (outillage IA/licences) et de préférence de
  stratégie de fusion.
- **#80 : fusionnable techniquement après #78 et après retarget vers `dev`.** Aucune objection
  technique ; vérifier le re-run CI post-retarget.
- **Arbre final attendu** : identique à `ea7200c` ; cumul 456 fichiers `+153 451/−13` ; 530
  fichiers suivis sur `dev`.

---

## 9. Limites de l'audit et non-claims

1. Audit **strictement read-only** : aucune fusion, aucun build, aucun test du dépôt exécuté, aucune
   commande git mutante ; les checks CI sont **observés** sur GitHub, pas re-exécutés par l'auditeur.
2. **Licences des dépendances non re-vérifiées** : pas de `cargo metadata` ni d'accès réseau dans le
   périmètre ; la claim antérieure « 41 deps permissives » (rapports MVP-0b) n'est pas re-mesurée.
3. La fusion est simulée **au niveau arbre** (`git merge-tree` ancien format, lecture seule) : elle
   établit 0 conflit pour les arbres actuels, pas une garantie contre une évolution future de `dev`.
4. Aucune analyse LFS côté serveur (aucun LFS utilisé), aucun scan CodeQL/SBOM/virus.
5. Les patterns de secrets scannés sont ceux listés au §4.1 ; un secret d'un format non couvert
   pourrait échapper au scan (limite intrinsèque).
6. `gh pr diff <n> --name-status` du brief n'existe pas dans `gh` ; vérification faite via
   `gh api …/files` + `git diff` local (résultats identiques : 186 / 304 fichiers).
7. Le constat initial « `dev` sans workflow » est **corrigé** : `dev` contient
   `.github/workflows/docs-check.yml` ; il ne contient en revanche aucun crate Rust.
8. Le HEAD local a avancé pendant l'audit et pendant la rédaction (`19ce0ab` → `95069fd` →
   `54177dd`), et le worktree contient des modifications (suivies ou non) d'autres workers
   (Cargo.lock, Cargo.toml, `tools/impact/selftest.mjs`, `crates/ravum-unit-graph/`,
   `crates/ravum-workspace-coordinator/src/{evaluation,oracle,records}.rs`,
   `crates/ravum-daemon/...`, `fixtures/...`) : **sans effet sur les PR #78/#80**, dont les têtes
   et bases sont restées inchangées pendant l'audit (`origin/*` revérifiés).
9. Aucune évaluation de performance/qualité fonctionnelle n'est faite ici : l'audit porte sur la
   structure, la classification, la sécurité de base et la fusionnabilité technique.

---

## Annexe A — Commandes de référence (toutes read-only) et sorties clés

```text
# Identité et accès
gh auth status                                  → compte Rwanbt, scopes gist/read:org/repo/workflow
git branch --show-current                       → feat/mvp0c-hardening
git rev-parse HEAD                              → 19ce0ab… puis 95069fd… (autre worker)

# PR / CI / protection
gh pr view 78 --json …                          → OPEN, MERGEABLE/CLEAN, 186 files, +25528/-6, 22 commits
gh pr view 80 --json …                          → OPEN, MERGEABLE/CLEAN, 304 files, +128220/-304, 54 commits
gh api repos/Rwanbt/Ravum/pulls/78 --jq .commits → 22
gh api repos/Rwanbt/Ravum/pulls/80 --jq .commits → 54
gh api repos/Rwanbt/Ravum/pulls/78/files --paginate --jq '.[].filename' → 186 noms (0 écart local)
gh api repos/Rwanbt/Ravum/pulls/80/files --paginate --jq '.[].filename' → 304 noms (0 écart local)
gh api repos/Rwanbt/Ravum/actions/runs/42529748229 → success, head ea7200c
gh api repos/Rwanbt/Ravum/actions/runs/42529748447 → success, head ea7200c
gh pr checks 78 / gh pr checks 80               → tous pass (docs, mvp, impact, auto-gc)
gh api repos/Rwanbt/Ravum/branches/dev/protection  → revue (0 approb.), conversation résolue, pas de force-push
gh api repos/Rwanbt/Ravum/branches/main/protection → required check "mvp"
gh api repos/Rwanbt/Ravum --jq '{allow_*}'      → merge/squash/rebase autorisés, delete_branch_on_merge=false

# Diffs et tailles
git diff --name-status 7841102 6b37ad3          → 186 lignes (A183, M3, D0)
git diff --name-status 6b37ad3 ea7200c          → 304 lignes (A269, M35, D0)
git diff --numstat --no-renames <base> <head>   → 0 entrée binaire
git diff --shortstat 7841102 6b37ad3            → 186 files changed, 25528 insertions(+), 6 deletions(-)
git diff --shortstat 6b37ad3 ea7200c            → 304 files changed, 128220 insertions(+), 304 deletions(-)
git diff --shortstat 7841102 ea7200c            → 456 files changed, 153451 insertions(+), 13 deletions(-)
git ls-tree -r -l <head>                        → tailles des blobs (classification §2, gros fichiers §3)
git diff --name-status -M <base> <head>         → 0 renommage

# Base dev
git rev-parse origin/dev origin/main            → 78411024… (identiques, 78 fichiers)
git ls-tree -r --name-only origin/dev | grep '^crates/'  → 0
git ls-tree -r --name-only origin/dev | grep '^\.github/' → 7 (dont workflows/docs-check.yml)

# Fusion (simulation lecture seule)
git merge-tree 64bd3ad… 6b37ad3… ea7200c…       → 0 conflit, 0 "changed in both", exit 0

# Sécurité / contenu
git diff <base> <head> | grep '^+' | analyse motifs secrets → 0 hit
git grep -l -I 'git-lfs' <heads>                → 0
git check-attr filter diff merge text -- <fichiers> → filter: unspecified
git show <head>:<fichier>                       → inspection ciblée workflows/scripts/skills/Cargo.toml
```

## Annexe B — Inventaire exhaustif

`evidence/mvp0c-pr-audit.json` contient, pour chaque PR :
`files.<pr>[] = { pr, status, category, path, bytes, additions, deletions }` et les agrégats
(`classification.<pr>.totals/categories`), les faits PR/CI/protection, les 34 chemins communs, les
fichiers volumineux, les résultats du scan de secrets/chemins personnels, les inventaires
workflows/dépendances/outillage IA/scripts et les limites.

Requêtes d'exemple :

```bash
jq '[.files."80"[] | select(.category=="Preuves (evidence)")] | {count:length, bytes:(map(.bytes)|add)}' evidence/mvp0c-pr-audit.json
jq '.files."78"[] | select(.bytes > 50000) | {path, category, bytes}' evidence/mvp0c-pr-audit.json
jq '.common_paths_78_80' evidence/mvp0c-pr-audit.json
```

## Annexe C — État des refs au moment T (2026-11-09 ~00:56 Europe/Paris)

```text
origin/dev                             78411024e2665dafce31b029786ef56d400bdab2
origin/main                            78411024e2665dafce31b029786ef56d400bdab2
origin/feat/3-mvp0a-continuous-loop    6b37ad3f22bce0ef0f30735ba0a4e32b6a31eda8   (tête PR #78)
origin/feat/mvp0b-hardening            ea7200c47991bc17044cf3cb50cbdd78c20005f9   (tête PR #80)
origin/feat/mvp0c-hardening            95069fd16eddc66a122fe47688cf91a2bb1e223c   (branche de travail, non auditée)
PR #78 : OPEN, MERGEABLE/CLEAN, base dev
PR #80 : OPEN, MERGEABLE/CLEAN, base feat/3-mvp0a-continuous-loop  ⚠️ retarget dev requis

Branche de travail locale (autre worker MVP-0c, hors périmètre de l'audit) :
  feat/mvp0c-hardening  19ce0ab → 95069fd (pendant l'audit) → 54177dd (observé après rédaction)
  → son évolution ne modifie ni origin/dev, ni les têtes/bases des PR #78/#80.
```
