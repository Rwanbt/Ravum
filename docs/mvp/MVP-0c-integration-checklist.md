# MVP-0c — Checklist de fusion et audit final d'intégration (L5.2)

> **Objet.** Donner au responsable une checklist **factuelle et vérifiée** pour décider de la fusion
> `#78 → #80 → #82` vers `dev`. Audit **read-only** : aucune commande git mutante, aucun build,
> aucun fichier déplacé ou supprimé. Chaque affirmation est reliée à une commande et à une sortie ;
> la preuve machine complète est dans
> [`evidence/mvp0c-integration-audit-2026-11-09.json`](../../evidence/mvp0c-integration-audit-2026-11-09.json).
>
> **Verdict : `READY_FOR_HUMAN_REVIEW`** — aucun bloquant technique identifié ; 4 constats
> non bloquants (§7) et des décisions humaines ouvertes (§8).

| Champ | Valeur |
|---|---|
| Tâche | `task-38` — MVP-0c L5.2 |
| Date de l'audit | 2026-11-09 03:34 → 03:50 (Europe/Paris) |
| Branche / HEAD | `feat/mvp0c-hardening` @ `24d92b1319112425aa690c0d3dff5df372ddfbf1` |
| HEAD local == `origin/feat/mvp0c-hardening` | **oui** (`24d92b1`) |
| Worktree au début de l'audit | **propre** (`git status --porcelain` vide) |
| PR auditées | #78 (OPEN), #80 (OPEN), #82 (**draft**) — toutes `MERGEABLE/CLEAN` |
| CI sur le HEAD audité | produit + impact **vertes** (runs `42551014521`, `42551014585`) |

## 1. État Git et GitHub vérifié

Commandes : `git status --porcelain`, `git rev-parse`, `git rev-list --count`, `git diff --shortstat`,
`git ls-tree`, `gh pr view`, `gh pr checks`, `gh api …/actions/runs`.

| Réf. | SHA |
|---|---|
| `origin/dev` = `origin/main` | `78411024e2665dafce31b029786ef56d400bdab2` |
| `#78` — `feat/3-mvp0a-continuous-loop` | `6b37ad3f22bce0ef0f30735ba0a4e32b6a31eda8` |
| `#80` — `feat/mvp0b-hardening` | `ea7200c47991bc17044cf3cb50cbdd78c20005f9` |
| `#82` — `feat/mvp0c-hardening` | `24d92b1319112425aa690c0d3dff5df372ddfbf1` |

| PR | État | Base | Fichiers | Lignes | Commits | Checks |
|---|---|---|---|---:|---:|---:|---|
| #78 | OPEN, draft non | `dev` | 186 | +25 528 / −6 | 22 | docs + mvp : **pass** |
| #80 | OPEN, draft non | `feat/3-mvp0a-continuous-loop` | 304 | +128 220 / −304 | 54 | mvp + impact + auto-gc : **pass** |
| #82 | OPEN, **draft oui** | `feat/mvp0b-hardening` | 202 | +71 322 / −273 | 31 | mvp + impact + auto-gc : **pass** |

- Commits au-dessus de `ea7200c` (tête #80) : **31** ; au-dessus de `dev` : **107** (22 + 54 + 31).
- Diff #82 vs `origin/feat/mvp0b-hardening` : **202 fichiers, +71 322/−273** ;
  cumul vs `origin/dev` : **630 fichiers, +224 500/−13**.
- CI du HEAD `24d92b1` : run `42551014521` (*MVP product checks*, success, 02:27:12Z) et
  `42551014585` (*MVP impact-selected checks*, success, 02:27:12Z) ; checks de PR
  `mvp` 4 m 58 s, `impact` 4 m 43 s, `auto-gc` 50 s.
- Protection de `dev` : conversation résolue exigée, force-push/suppression interdits,
  **0 approbation requise** et **aucun required status check** (`required_status_checks: null`) —
  la CI doit donc être vérifiée humainement (§8).

## 2. Inventaire du diff #82 (base `origin/feat/mvp0b-hardening`)

Commandes : `git diff --name-status`, `git diff --numstat --no-renames`, `git ls-tree -r -l`,
`git diff --name-status -M` (renommages), `git diff --name-only` (comptage).

| Catégorie | Fichiers | Octets (blobs tête) | Lignes | Statuts |
|---|---:|---:|---:|---|
| Preuves (evidence) | 33 | 1 333 162 | +38 485 / −0 | A33 |
| Code produit Rust | 30 | 762 620 | +11 171 / −252 | A14, M16 |
| Documentation | 20 | 391 612 | +6 418 / −0 | A19, M1 |
| Tests Rust | 18 | 335 748 | +8 577 / −1 | A17, M1 |
| Benchmarks / harness | 17 | 191 327 | +4 491 / −0 | A17 |
| Outils build/impact | 3 | 125 142 | +620 / −11 | A1, M2 |
| Fixtures | 69 | 36 355 | +1 268 / −0 | A69 |
| CI / workflows | 1 | 18 172 | +61 / −7 | M1 |
| Scripts / outillage | 3 | 16 719 | +138 / −2 | A1, M2 |
| Lockfile (généré) | 1 | 11 808 | +32 / −0 | M1 |
| Manifests Cargo | 6 | 4 589 | +58 / −0 | A3, M3 |
| Config / scripts divers | 1 | 560 | +3 / −0 | M1 |
| **Total** | **202** | **3 227 814** | **+71 322 / −273** | **A174, M28** |

- **Aucun fichier supprimé** (`D = 0`), **aucun renommage**, **aucun binaire**
  (`git diff --numstat` sans entrée binaire).
- **Aucun fichier > 300 Ko ni > 1 Mo** ; top 5 : `evidence/mvp0c-s8-pilot` 226 569 B,
  `evidence/mvp0c-verification-transient` 194 895 B, `evidence/mvp0c-grouping` 164 327 B,
  `evidence/mvp0c-grouping-multiagent` 148 531 B, `evidence/mvp0c-pr-audit` 146 562 B.
- **Aucun fichier généré suivi** (`target/`, `evidence/local/`, `impact.json`, `*.log`,
  `node_modules/`, `.env`) : 0 dans tout l'arbre `24d92b1`.
- **Doublons** : 3 groupes de blobs identiques, tous dans des **fixtures volontaires**
  (`fixtures/collab5/cases/{01,08,10}/src/lib.rs`, deux `a.patch` identiques,
  `fixtures/{multicrate,unit-graph-rootapp}/core/Cargo.toml`) — pas des copies accidentelles.
- Aucun chemin hors dépôt (absolu, `..`, lecteur) dans les fichiers du diff.

## 3. Hygiène : secrets, chemins personnels, workflows, outillage IA

Commandes : `git diff … | filtre motifs`, `git grep`, `git diff` ciblés.

- **Secrets : 0 détecté** sur les 71 305 lignes ajoutées (motifs `ghp_/gho_/ghs_/ghu_/ghr_`,
  `github_pat_`, `sk-`/`sk-ant-`, `AKIA…`, `xox…`/`AIza…`, bloc de clé privée, JWT,
  affectations `secret|password|token`, `${{ secrets.* }}`). Les 3 occurrences de `${{ secrets.`
  sont du **texte de documentation** (l'audit PR) et **non** des workflows.
- **Chemins personnels** : 201 occurrences de type `C:\Users\<utilisateur>` dans **14 preuves
  JSON** nouvelles, 11 occurrences `<lecteur>:\App\<projet>` dans 7 fichiers — **non bloquant**,
  à redacter si les preuves sont republiées hors du dépôt (aucune valeur secrète).
- **Workflow modifié** : `.github/workflows/mvp-impact.yml` (+61/−7) — ajoute un selftest Node
  obligatoire du mapping, rend les **E2E navigateur requis fail-closed** (`browser_e2e_required`),
  étend le fallback JSON, upload d'artefact E2E. `permissions: contents: read` inchangées,
  aucun secret, pas de `pull_request_target`. Actions épinglées par **tag majeur** (pas SHA).
- **Outillage IA tiers** : **aucun fichier dans le diff #82** ; il est hérité de #78
  (47 fichiers/337 775 B, skills copiés d'AI-Native-Dev-Stack/gstack sans notice de licence,
  duplication `.claude`/`.agents`) — voir
  [MVP-0c-pr-audit.md](MVP-0c-pr-audit.md) §4.4/4.5. Décision de gouvernance dans §8.
- **Scripts** : 3 fichiers (A1/M2), aucune nouvelle opération destructive ; les scripts de
  démonstration continuent de travailler sous `target/` (dépôts jetables).

## 4. Documentation et preuves

Commandes : extraction des liens Markdown des 21 documents modifiés + `git cat-file -e` ;
`git show HEAD:<json> | ConvertFrom-Json` pour les 33 preuves JSON.

- **Liens relatifs : 76 vérifiés, 0 cassé** (liens externes et ancres exclus du contrôle de fichier).
- **Preuves JSON : 33 fichiers modifiés/ajoutés, 33 valides, 0 invalide.**
- **Preuves > 100 Ko** : 5 fichiers, max 226 569 B (aucune > 300 Ko) — LFS non requis.
- **Constats documentaires non bloquants** :
  - le rapport affirme **491 tests / 0 échec / 77 suites**
    ([STATUS.md](../architecture/STATUS.md) ligne 32 ;
    [MVP-0c-REPORT.md](MVP-0c-REPORT.md) lignes 20/197/357), alors que la CI du HEAD audité
    compte **492 passés / 0 échec** sur 78 lignes de suite (23 suites à 0 test) : écart de
    **+1 test / +1 ligne** ;
  - `MVP-0c-REPORT.md` §17 cite `evidence/mvp0c-pr-audit-2026-11-09.json` ; le fichier réel est
    [`evidence/mvp0c-pr-audit.json`](../../evidence/mvp0c-pr-audit.json) (sans suffixe de date) —
    15 des 16 preuves citées existent, 1 nom à corriger.

> **Résolution (Lead, même session).** L'écart +1 est un artefact de log : la CI exécute la suite workspace (**491 tests**) puis un test filtré lancé par une démo (migration v1→v2, 9 filtered out, 1 passed), d'où 492 résultats dans le log complet du job. Le bon compteur pour la suite workspace reste **491/0/77**. Le nom de la preuve d'audit initiale est corrigé dans le rapport (§17).

## 5. Tests : aucun supprimé ni désactivé

Commandes : `git diff` filtré sur `#[test]`, `#[ignore]`, `todo!`, `unimplemented!`,
fichiers sous `tests/`, et assertions.

- **0 fichier de test supprimé**, **0 `#[test]` retiré**, **0 `#[ignore]` ajouté**,
  **0 `todo!`/`unimplemented!`** ajouté.
- 11 lignes d'assertion supprimées (3 fichiers) contre **988 lignes d'assertion ajoutées** :
  - `crates/ravum-daemon/tests/auto_gc_integration.rs` : l'assertion immédiate est remplacée par
    une **attente bornée** (`wait_for_auto_gc_run`, 10 s) qui échoue avec diagnostics complets,
    puis l'assertion de settle est conservée — renforcement, pas affaiblissement ;
  - `crates/ravum-workspace-coordinator/src/coordinator.rs` : tests réorganisés, +75 assertions
    dans le fichier ;
  - `crates/ravum-cli/src/collab_cycle.rs` : une assertion remplacée dans un test réécrit.
- Le compteur global **492/0** du HEAD est cohérent avec la CI (écart documentaire §4).

## 6. Séquence de fusion proposée — **à exécuter par l'humain**, non exécutée

```bash
# 0) Pré-vol (lecture seule)
git fetch origin
git rev-parse origin/dev origin/feat/3-mvp0a-continuous-loop \
              origin/feat/mvp0b-hardening origin/feat/mvp0c-hardening
git status --porcelain                 # doit être vide
gh pr checks 78 && gh pr checks 80 && gh pr checks 82

# 1) #78 -> dev (merge commit ; préserve l'ascendance nécessaire à #80)
gh pr merge 78 --merge --match-head-commit 6b37ad3f22bce0ef0f30735ba0a4e32b6a31eda8
git fetch origin
test "$(git rev-parse 'origin/dev^{tree}')" = "64bd3ad235d2846702ab5487c358df104e391712"
git diff --shortstat 7841102..origin/dev          # attendu : 186 files, +25528/-6
git ls-tree -r --name-only origin/dev | wc -l     # attendu : 261

# 2) retarget #80 -> dev (sa base est encore la branche de #78)
gh pr edit 80 --base dev
gh pr view 80 --json baseRefName,mergeable,mergeStateStatus

# 3) #80 -> dev
gh pr merge 80 --merge --match-head-commit ea7200c47991bc17044cf3cb50cbdd78c20005f9
git fetch origin
test "$(git rev-parse 'origin/dev^{tree}')" = "030e6b0e9c690d6e88cf865f4ceef9b8bebb5bc8"
git diff --shortstat <M78>..origin/dev            # attendu : 304 files, +128220/-304
git ls-tree -r --name-only origin/dev | wc -l     # attendu : 530

# 4) #82 : la rendre prete puis la retargeter vers dev (base actuelle = branche de #80)
gh pr ready 82
gh pr edit 82 --base dev
gh pr view 82 --json baseRefName,isDraft,mergeable,mergeStateStatus

# 5) #82 -> dev
gh pr merge 82 --merge --match-head-commit 24d92b1319112425aa690c0d3dff5df372ddfbf1
git fetch origin
test "$(git rev-parse 'origin/dev^{tree}')" = "b62b9ae9ca104c6ed53c5e5f6d349748fccd9d7e"
git diff --shortstat <M80>..origin/dev            # attendu : 202 files, +71322/-273
git diff --shortstat 7841102..origin/dev          # cumul : 630 files, +224500/-13
git ls-tree -r --name-only origin/dev | wc -l     # attendu : 704

# 6) Vérification post-fusion
gh pr checks 82
gh api "repos/Rwanbt/Ravum/actions/runs?head_sha=$(git rev-parse origin/dev)"
# attendu : CI produit + impact vertes sur dev ; docs integrity ; aucun conflit
```

- **Simulation read-only** (ancien `git merge-tree`, aucun objet écrit) :
  `git merge-tree 030e6b0e9c690d6e88cf865f4ceef9b8bebb5bc8 origin/feat/mvp0b-hardening origin/feat/mvp0c-hardening`
  → 73 884 lignes de sortie, **0 marqueur de conflit**, 0 en-tête `changed in both`/`added in both`.
- Arbres attendus : `dev` `28cde46…` → après #78 `64bd3ad…` → après #80 `030e6b0e…` →
  après #82 `b62b9ae…` ; fichiers suivis : 78 → 261 → 530 → **704**.
- Les sommes par PR (+25 528 + 128 220 + 71 322 = 225 070) diffèrent du cumul (+224 500) car
  des fichiers sont modifiés par plusieurs PR : c'est normal, le cumul fait foi.
- Si `dev` bouge entre-temps (aucune autre PR ouverte au moment de l'audit), rejouer
  `git merge-base origin/dev <tête>` et la simulation avant chaque fusion.

## 7. Risques et constats (aucun bloquant)

| ID | Sévérité | Constat | Mitigation |
|---|---|---|---|
| R1 | process | PRs **empilées** : #82 → branche #80, #80 → branche #78 ; #82 est **draft** ; retarget obligatoire | suivre §6, vérifier `baseRefName`/`mergeable`/checks avant chaque merge |
| R2 | process | Protection de `dev` **sans required status check** (0 approbation, conversation résolue) | vérifier humainement mvp+impact+docs vertes ; envisager de les rendre requis |
| R3 | gouvernance | Décisions ouvertes : licence (aucune), outillage IA tiers hérité, 42 ADR + 2 RFC `PROPOSED` (43 fichiers dans `docs/adr`, README inclus), ASIO/Kani/S8/release | cases §8 et [MVP-0c-REPORT.md](MVP-0c-REPORT.md) §16 |
| R4 | taille | 202 fichiers/+71 k lignes pour #82 ; cumul 630 fichiers/+224,5 k lignes ; preuves 1,33 Mo (max 226 Ko) | aucune action technique requise ; revue par lots si souhaitée |
| R5 | hygiène | 201 chemins personnels dans 14 preuves ; compteur 491 documenté vs 492 CI ; preuve citée avec un nom légèrement faux | corrections documentaires optionnelles |
| R6 | CI | `mvp-impact.yml` modifié (selftest + E2E requis) ; actions épinglées par tag majeur ; impact 4 m 43 | acceptable ; épinglage SHA en durcissement ultérieur |
| R7 | workspace | modifications locales non liées (`fixtures/grouping`) apparues **pendant** l'audit, hors PR | vérifier `git status` vide juste avant la séquence |

## 8. Cases à cocher du responsable

- [ ] **Marquer #82 « ready for review »** (`gh pr ready 82`) et relire le périmètre du diff
      (202 fichiers ; rapport [MVP-0c-REPORT.md](MVP-0c-REPORT.md)).
- [ ] **Approuver la revue** des trois PR (la protection de `dev` n'exige aucune approbation :
      la décision reste humaine).
- [ ] **Décider de la licence** du code et de la gouvernance de l'outillage IA tiers commité
      (aucune licence choisie ; voir [MVP-0c-REPORT.md](MVP-0c-REPORT.md) §16.2 et
      [MVP-0c-pr-audit.md](MVP-0c-pr-audit.md) §4.4/4.5).
- [ ] **Ratifier ou rejeter les ADR/RFC** (42 ADR + 2 RFC, tous `PROPOSED` ; 43 fichiers dans
      `docs/adr` avec le README ; aucune acceptation automatique) —
      [MVP-0c-REPORT.md](MVP-0c-REPORT.md) §16.3.
- [ ] **Décider du release/déploiement** (aucune release ; aucun ADR accepté) —
      [MVP-0c-REPORT.md](MVP-0c-REPORT.md) §1 et §16.
- [ ] **Décider des chantiers non exécutés** : ASIO (licence SDK), hôte CLAP/VST3, Kani/Linux/WSL2,
      campagne S8 avec vrais agents, opt-level=0 — [MVP-0c-REPORT.md](MVP-0c-REPORT.md) §14–§16 et
      [MVP-0c-qualification.md](MVP-0c-qualification.md).
- [ ] **Vérifier le worktree propre** juste avant la séquence : des modifications locales non liées
      sont apparues **pendant** l'audit (liste non exhaustive : `M fixtures/grouping/src/lib.rs`,
      `?? fixtures/grouping/…`, `?? crates/ravum-daemon/src/bin/ravum-daemon-bench/batch.rs`,
      `M docs/mvp/MVP-0c-qualification.md`, `?? evidence/mvp0c-impact-e2e-ci-2026-11-09.json`),
      hors PR #82.
- [ ] **Exécuter la séquence §6** dans l'ordre, en vérifiant à chaque étape arbre, compteurs,
      `git status`, checks GitHub ; **ne pas supprimer** la branche de base de #82 avant son
      retarget (sinon GitHub retargerait vers `main`).
- [ ] **Vérifier après chaque fusion** : CI produit + impact vertes sur `dev`, arbre attendu,
      fichiers comptés, et `dev` protégée avec la politique retenue.
- [ ] **Nettoyer les branches** après fusion (les trois branches peuvent être supprimées **après**
      les vérifications ; `delete_branch_on_merge=false`).

## 9. Verdict et limites

**Verdict : `READY_FOR_HUMAN_REVIEW`** — aucun bloquant technique : PRs `MERGEABLE/CLEAN`,
CI verte sur les têtes (#78, #80, #82), 0 conflit simulé, 0 secret, 0 test supprimé/désactivé,
0 fichier généré suivi, tous les liens relatifs des docs valides (76/76), 33/33 preuves JSON
valides, aucun fichier > 300 Ko ni binaire. Les 4 constats non bloquants (§4, §7 R5/R7) sont
documentaires ou d'environnement local.

**Limites de cet audit** : aucun build/test re-exécuté (les compteurs viennent du log CI du run
`42551014521` sur `24d92b1`) ; la fusion est simulée au niveau arbre (`git merge-tree`) pour les
réfs actuelles et doit être rejouée si `dev` évolue ; les liens Markdown ne sont vérifiés qu'en
existence de fichier (pas de titres/ancres) ; les secrets sont recherchés sur les lignes ajoutées
avec les motifs listés (§3) ; la répartition des tests par crate n'est pas re-mesurée
(seul le total 492/0 est vérifié) ; les preuves JSON citées par le rapport sont vérifiées
fichier par fichier, à l'exception du nom `mvp0c-pr-audit-2026-11-09.json` signalé.

## 10. Résolutions post-audit (9 novembre, Lead)

- **Formatage** : résolu — workspace formaté avec rustfmt 1.95, toolchain épinglée
  `1.95.0` (`rust-toolchain.toml`, CI `dtolnay/rust-toolchain@1.95.0`) et gate
  `cargo fmt --all -- --check` ; suite 491/0/77 après formatage. L'écart CI
  (rustc 1.99.1 vs 1.95.0 documenté) est supprimé par l'épinglage.
- **Chemins personnels** : résolu — 63 preuves JSON anonymisées (124 chemins
  utilisateur, 739 chemins de dépôt, 124 chemins temporaires), aucune occurrence du
  nom d'utilisateur dans les fichiers suivis ; manifeste
  `evidence/mvp0c-evidence-sanitization-2026-11-09.json`. Les tailles de preuves
  citées au §4 peuvent avoir légèrement diminué ; l'ordre de grandeur et les
  conclusions sont inchangés.
- **Fusion re-simulée** : `git merge-tree` avec les bases actuelles (`dev`
  `045b285`, #78 `550847e`, #80 `45d8434`, #82 `e9f5aab`) → **0 conflit** ;
  arbres `975fecb9…`, `dbb0a69d…`, `5617d996…`. Ces arbres doivent être rejoués si
  `dev`/les têtes bougent encore ; la séquence §6 reste valable.
- **Verdict inchangé** : `READY_FOR_HUMAN_REVIEW` ; restent ouverts la décision
  humaine de fusion, la protection `dev`, le câblage produit du collecteur, Kani et
  ASIO/CLAP.
