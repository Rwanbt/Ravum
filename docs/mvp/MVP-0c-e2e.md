# MVP-0c / L4.4 — E2E navigateur revalidés et trou de couverture CI

> **Portée** : revalider sur l'état MVP-0c les 10 scénarios E2E navigateur du MVP-0b,
> vérifier leur exécution réelle dans la CI, analyser le trou de couverture
> `apps/** => E2E navigateur` dans `mvp-impact.yml`, et proposer une règle de
> branchement **sans modifier les workflows**.
>
> **Statut des preuves** : `TESTED` localement le 2026-11-09 (Windows 11, Chromium 156
> headless) et `TESTED` en CI sur deux commits (runs réels ci-dessous). Ce document ne
> qualifie ni le produit Ravum, ni la stabilité ABI, ni le rendu visuel. Aucune preuve
> n'est `PROVED`.
>
> **Provenance** : dépôt `Rwanbt/Ravum`, branche `feat/mvp0c-hardening`.
> Commit demandé `58220f5` ; la branche a avancé pendant la mission et l'exécution
> locale a eu lieu sur `05350c7`. Le sous-arbre E2E/app/workflows/outil d'impact est
> **identique** entre les deux (`git diff --stat 58220f5..05350c7 -- apps/ravum-todo
> .github/workflows tools/impact` → vide). La CI demandée (run 42538867282) porte sur
> `58220f5` ; un run CI sur `05350c7` est également vert. Détail complet dans
> `evidence/mvp0c-e2e-2026-11-09.json`.
>
> Aucune commande git mutante, aucun workflow ni source d'application modifié, aucun
> test supprimé/désactivé, aucun paquet npm.

## 1. Résultat en une phrase

Les **10 scénarios E2E navigateur passent réellement** (`10/10 PASS`, `44`
vérifications décrites, `0 skip`) localement sur l'état MVP-0c et en CI
(`Chrome/156.0.8078.80` sur `windows-latest`), mais **`mvp-impact.yml` ne les exécute
jamais** : un diff `apps/**` y produit uniquement `cargo test -p ravum-todo`, sans
navigateur. La règle de branchement proposée coûte **≈ 25 à 65 s** par exécution
déclenchée et ne s'applique qu'aux chemins UI/contrat/schéma de l'app.

## 2. Tableau scénario → couverture → résultat local → CI

| # | Scénario (contrat/API couvert) | Vérifié dans le DOM | Local (Chromium 156 headless) | CI (Chrome 156, run 42538867282) |
|---|---|---|---|---|
| 1 | Chargement UI + contrat (`/api/schema`, `contract_version=1`) | titre, `h1`, formulaire (jeton, titre `required`, échéance, bouton), liste vide, bandeau « Contrat v1 vérifié. » | **PASS** (500 ms) | **PASS** (4 105 ms) |
| 2 | Création (`POST /api/tasks`) | saisie + clic, ligne dans `#tasks`, boutons Modifier/Terminer/Supprimer, champ vidé | **PASS** (494 ms) | **PASS** (677 ms) |
| 3 | Modification du titre (`PATCH {title}`) | éditeur inline, valeur remplacée, DOM re-rendu | **PASS** (598 ms) | **PASS** (1 813 ms) |
| 4 | Suppression (`DELETE`) | disparition de la ligne, « aucune tâche », 0 tâche en base | **PASS** (552 ms) | **PASS** (1 245 ms) |
| 5 | Validation (400, `title_empty`) | message DOM titre vide (garde HTML), message serveur pour `"   "`, aucune tâche créée | **PASS** (429 ms) | **PASS** (1 375 ms) |
| 6 | Autorisation (401) | jeton invalide → erreur affichée, page vivante ; jeton absent → garde cliente ; 0 exception non capturée | **PASS** (476 ms) | **PASS** (1 158 ms) |
| 7 | Contrat incompatible (`contract_version` ≠ 1) | bandeau « Client incompatible… Mutations désactivées », bouton désactivé, lecture toujours rendue, aucune mutation | **PASS** (844 ms) | **PASS** (1 815 ms) |
| 8 | Persistance (redémarrage même base) | tâche toujours dans le DOM après relance + rechargement | **PASS** (800 ms) | **PASS** (999 ms) |
| 9 | Migration v1 → v2 (`SCHEMA_VERSION=2`) | ligne v1 visible, `/health` schema 2, nouvelle tâche `due_date` affichée et persistée | **PASS** (501 ms) | **PASS** (914 ms) |
| 10 | Erreur réseau (serveur arrêté) | « Failed to fetch » affiché, page vivante, **0** `Runtime.exceptionThrown` | **PASS** (1 082 ms) | **PASS** (976 ms) |

Totaux :

- local : `10/10 PASS`, `0 échec`, `0 skip`, `44` vérifications, **7,4 s** de harnais
  (12,5 s porte-à-porte), `exit 0`, 10 captures ;
- CI `58220f5` : `10/10 PASS`, `0 échec`, `44` vérifications, **39,3 s** de harnais,
  rapport `report.json` publié en artefact `browser-e2e-evidence` ;
- CI `05350c7` : `10/10 PASS`, `44` vérifications, **14,5 s** de harnais (runner chaud).

Les erreurs attendues (HTTP 400/401, `ERR_CONNECTION_REFUSED`, `ERR_ABORTED` de
comptabilité Chromium sur `DELETE 204`) sont déclarées par scénario ; tout le reste
échoue. Aucun scénario, aucune assertion et aucune capture n'a été supprimé ou
désactivé.

## 3. Revalidation locale (exécution réelle)

```powershell
# À la racine du dépôt, Node 26 ; binaire de l'app compilé depuis HEAD
$env:RAVUM_TODO_BIN = "target\debug\ravum-todo.exe"
$env:RAVUM_E2E_REQUIRE_BROWSER = "1"
node apps/ravum-todo/e2e/browser_e2e.mjs `
  --screenshots=<temp>\screenshots --report=<temp>\local-report.json
# EXIT=0, 10/10 PASS, 44 vérifications, 7,4 s (harnais), 12,5 s (processus)
```

Navigateur réellement découvert : headless shell Playwright
`chromium_headless_shell-1248\chrome-headless-shell-win64\chrome-headless-shell.exe`
→ `HeadlessChrome/156.0.8078.4`, protocole CDP 1.3. Node `v26.13.0`,
Windows 11 Pro `10.0.26100`. Serveur réel démarré par scénario sur un port libre, vrai
socket, vrai fichier SQLite temporaire. Les captures et le rapport sont dans
`%TEMP%\ravum-mvp0c-e2e-final\` (hors dépôt).

**Contournement d'environnement, déclaré** : sur cette machine partagée et chargée,
`taskkill /PID … /T /F` prend **61,5 s et échoue** (exit 1, processus toujours vivant),
même sur un `cmd.exe` nu ; `Stop-Process -Force` tue en **0,02 s**. Sans correctif, chaque
scénario payait ~2 min de nettoyage et le run dépassait 20 min. L'exécution locale a donc
utilisé un préchargement Node **hors dépôt** (`NODE_OPTIONS=--require=<temp>/taskkill-preload.cjs`)
qui remplace les appels `spawnSync('taskkill', …)` du harnais par `process.kill(pid)`
(`TerminateProcess`). Le harnais, les scénarios et l'application ne sont pas modifiés ;
le serveur est lancé via `RAVUM_TODO_BIN` (enfant direct, sans descendants), donc la
sémantique de nettoyage requise est équivalente. Le log compte **12** lignes
`[taskkill-shim]` : 11 arrêts de serveur + 1 arrêt navigateur. **La CI utilise le
`taskkill` normal** et n'a pas ce problème (étapes E2E vertes en 20,6 s et 45,7 s).

## 4. Vérification des logs CI (run demandé et run courant)

### 4.1 Run demandé — `42538867282` sur `58220f5`

- Workflow `.github/workflows/mvp.yml`, événement `push`, conclusion **success**
  (`https://github.com/Rwanbt/Ravum/actions/runs/42538867282`).
- Job `mvp` **ID 128909135716**, 6 min 8 s.
- Étape **15** « Browser E2E - real Chromium/Edge via CDP (no npm dependency) » :
  **conclusion success**, fenêtre UTC `00:17:06.832 → 00:17:52.527` = **45,7 s** ;
  étape 14 « Setup Node.js » success (18,9 s) ; étape 16 « Upload browser E2E
  evidence » success.
- Artefact `browser-e2e-evidence` : `report.json` indique **Chrome/156.0.8078.80**
  (`C:\Program Files\Google\Chrome\Application\chrome.exe`),
  Node `v24.22.2`, Windows Server 2025 Datacenter, `10/10`, `44` vérifications,
  `durationMs=39256`, `harnessError=null`, commit `58220f5`.
- Log E2E extrait : navigateur Chrome 156, `Résultat: 10/10 scénarios réussis,
  0 échec(s), durée 39.3 s`, `SUCCÈS`. Conclusion : **l'étape E2E s'est bien exécutée
  et a réussi ; ce n'était ni un skip ni un faux vert.**

### 4.2 Run courant — `42541321576` sur `05350c7`

Même workflow, même étape 15 **success** (fenêtre `00:43:03.198 → 00:43:23.815` =
**20,6 s**), artefact `10/10`, `44` vérifications, `durationMs=14547`.

### 4.3 Workflow d'impact sur les mêmes commits

- `42538867273` (`58220f5`) : job `impact` **ID 128909135467**, 2 min 12 s, **success**,
  décision `full-conservative` (le diff de push vs `origin/dev` cumule les commits de
  durcissement CI). **Aucune étape navigateur** ; étape 11 « Run exactly the required
  Rust tests » *skipped*, étape 10 suite FULL exécutée.
- `42541321622` (`05350c7`) : `impact`, 3 min 32 s, success, FULL conservateur, aucune
  étape navigateur.
- Le selftest du mapping (étape 6 de `mvp-impact.yml`, exécutée avant toute décision)
  passe en CI. Sur l'arbre de travail **local** partagé, le même selftest échoue sur
  2 cas parce qu'un autre chantier y a ajouté des `dev-dependencies` non commitées à
  `ravum-dsp-harness` ; sur un export propre de `HEAD` (`git archive`), le selftest
  passe : **52 cas, 505 assertions, 0 échec**. Ce n'est pas une régression du commit.

## 5. Le trou de couverture CI, prouvé

### 5.1 Constat

1. `.github/workflows/mvp-impact.yml` **n'a aucune étape navigateur** : pas de
   `setup-node`, pas de `demo-m4-browser.ps1`, pas d'artefact E2E (vérifié : aucune
   occurrence de `browser|e2e|setup-node|playwright|chrome`, insensible à la casse).
2. `tools/impact/impact.mjs` classe tout `apps/<membre>/**` en `pkg:<membre>` et n'émet
   que des commandes `cargo` (`cargo test -p … -- --test-threads=1`, ou
   `cargo test --workspace -- --test-threads=1` en FULL). Sa limite est écrite noire sur
   blanc (ligne 79) : « UI statique (`apps/*/static/**`) et suites E2E navigateur
   éventuelles : la suite Rust du paquet est exigée, **le navigateur n'est pas exécuté
   ici** ».
3. `docs/mvp/MVP-0b-ci.md` documentait déjà la limite (§3 lignes 102-123, §7 point 6) :
   un futur branchement devra être ajouté au mapping.

### 5.2 Simulations de diffs (même outil, même HEAD, sorties JSON)

| Diff simulé | Décision | Commandes exigées | E2E navigateur ? |
|---|---|---|---|
| `apps/ravum-todo/static/app.js` | sélective | `cargo test -p ravum-todo -- --test-threads=1` | **non** |
| `apps/ravum-todo/src/routes.rs, apps/ravum-todo/static/index.html` | sélective | `cargo test -p ravum-todo …` | **non** |
| `apps/ravum-todo/e2e/browser_e2e.mjs` | sélective | `cargo test -p ravum-todo …` | **non** |
| `crates/ravum-todo-core/src/lib.rs` | sélective | tests `ravum-todo-core`, `ravum-store`, `ravum-todo` | **non** |
| `apps/ravum-unknown/x.js` | FULL conservateur + `unknown_paths` | `cargo test --workspace -- --test-threads=1` | **non** |
| `apps/ravum-todo/src/routes.rs, scripts/demo-m4-browser.ps1` | FULL conservateur | `cargo test --workspace -- --test-threads=1` | **non** |

**Conséquence exacte** : le job `impact` peut être vert sans avoir exécuté une seule
ligne du JavaScript de l'UI. Aujourd'hui, sur `push feat/**` et sur PR vers
`main`/`dev`, `mvp.yml` exécute aussi les E2E : la couverture existe donc *grâce à
`mvp.yml`*, pas grâce au check d'impact. Le trou devient un risque réel si
`mvp-impact` est le **seul check requis** (protection de branche), sur un **PR de fork**
où `mvp.yml` n'est pas exécuté, ou si `mvp.yml` est rendu conditionnel. Un FULL
conservateur (ex. `scripts/**`) n'y remédie pas : il exécute `cargo test --workspace`,
pas les démos/E2E de `mvp.yml`.

## 6. Règle de couverture proposée (non appliquée)

> Les workflows n'ont **pas** été modifiés : cette proposition est à arbitrer et à
> implémenter par le Lead (hors périmètre de cette mission).

**Règle** : exposer depuis `impact.mjs` un booléen `browser_e2e_required` et exécuter
l'étape navigateur dans `mvp-impact.yml` quand il est vrai.

`browser_e2e_required = true` si l'un des cas suivants :

1. un chemin modifié est sous `apps/ravum-todo/**` (src, static, e2e inclus) ;
2. un chemin modifié est sous `apps/*/static/**` ou `apps/*/e2e/**` (règle générique
   pour toute application future avec UI) ;
3. un chemin modifié est sous `crates/ravum-todo-core/**` (contrat partagé servi à
   l'UI) ou `crates/ravum-store/**` (schéma SQLite observé par l'UI, scénario migration) ;
4. un chemin modifié est `scripts/demo-m4-browser.ps1` (lanceur du harnais) ;
5. `decision.uncertain === true` (fail-closed : mapping non certifié => exécuter aussi
   le navigateur) ;
6. `decision.full === true` **et** (`Cargo.toml`/`Cargo.lock` ou un chemin des cas 1–4
   est modifié) : un manifeste peut changer le binaire de l'app.

Câblage proposé dans `mvp-impact.yml` (après « Compute impact mapping ») :

```yaml
# (proposition — non appliquée dans cette mission)
- uses: actions/setup-node@v4
  if: steps.impact.outputs.browser_e2e == 'true'
  with: { node-version: '24' }
- name: Browser E2E (apps/UI affected)
  if: steps.impact.outputs.browser_e2e == 'true'
  run: powershell -ExecutionPolicy Bypass -File scripts/demo-m4-browser.ps1 -RequireBrowser -Screenshots
- uses: actions/upload-artifact@v4
  if: always() && steps.impact.outputs.browser_e2e == 'true'
  with: { name: browser-e2e-evidence, path: target/ravum/browser-e2e }
```

**Coût mesuré** (pas modélisé) : étape E2E **20,6 s à 45,7 s** + `setup-node`
**4,5 s à 18,9 s** ⇒ **≈ 25 à 65 s** ajoutées *uniquement quand la règle se déclenche* ;
0 s sur un diff docs/evidence-only. Jobs impact actuels : 2 min 12 s (`58220f5`) et
3 min 32 s (`05350c7`) ; pire cas estimé ≈ 4 min 40 s, très loin du timeout de 40 min.
Une exécution E2E inconditionnelle coûterait ce temps sur *tous* les diffs ; la règle
ciblée évite ce surcoût.

**Alternative** : garder les E2E uniquement dans `mvp.yml` et assumer que `mvp-impact`
n'est pas un gate UI. Moins coûteux, mais la promesse « exactement les tests requis »
reste incomplète et la couverture dépend d'un second workflow.

## 7. Aucun nouveau scénario nécessaire

Depuis le commit MVP-0b `c2c2ce9`, la seule évolution de l'application est le **signal
de disponibilité** ajouté dans `static/app.js` (commit `7bc128a`, 7 lignes) pour rendre
les E2E déterministes ; `git diff --stat c2c2ce9..HEAD -- apps/ravum-todo/src
apps/ravum-todo/Cargo.toml crates/ravum-todo-core crates/ravum-store` est **vide**.

- `CONTRACT_VERSION = 1` (`ravum-todo-core`) et `SCHEMA_VERSION = 2` (`ravum-store`)
  sont inchangés ;
- routes inchangées (`/health`, `/api/schema`, `/api/tasks`, `/api/tasks/{id}`, `/`, `/app.js`) ;
- validations et champs inchangés ;
- le signal `data-ravum-ready` est déjà consommé par `page.waitForReady()` dans les
  scénarios concernés : aucun comportement produit nouveau à tester.

Les 10 scénarios restent donc représentatifs du contrat/API courant. Aucun fichier du
harnais n'a été modifié ni ajouté : la revalidation porte sur le harnais existant, tel
qu'exécuté en CI. (Le périmètre autorisait des additions ; aucune n'était justifiée.)

## 8. Limites honnêtes

1. **Headless ≠ navigateur de bureau** : pas de fenêtre, pas de GPU/compositeur visible,
   pas de gestes utilisateur réels au-delà de `.click()` DOM ; le rendu pixel n'est pas
   asserté.
2. **Un seul moteur** : Chromium 156 (headless shell local, Chrome en CI). Firefox,
   WebKit/Safari, Edge complets et mobile ne sont pas testés.
3. **Pas de mobile/responsive/tactile**, pas d'accessibilité (rôles, labels, lecteur
   d'écran), pas de régression visuelle.
4. **Pas de TLS/HTTPS**, pas de multi-utilisateur concurrent, pas de charge, pas de
   coupure réseau partielle ; le serveur de démonstration fait une requête par connexion.
5. **Exécution locale sous charge partagée** : shim `taskkill` hors dépôt déclaré au
   §3 ; la CI utilise `taskkill` normal (preuves vertes). Le run local a eu lieu sur
   `05350c7` (branche avancée pendant la mission), sous-arbre E2E identique à `58220f5`.
6. **Selftest local du mapping** : échoue sur l'arbre de travail à cause de changements
   non commités d'un autre chantier ; passe sur l'export propre de `HEAD` et en CI.
   Fragilité connue : le selftest lit le disque, pas le commit.
7. **`scripts/impact.ps1`** (lanceur local) échoue sous Windows PowerShell 5.1 à cause
   d'un encodage UTF-8 sans BOM ; la CI appelle `node tools/impact/impact.mjs`
   directement, donc aucun impact CI. À corriger hors périmètre si le lanceur local
   doit rester supporté.
8. **Pas de diff réel purement apps-only observé en CI** : les runs de push comparent à
   `origin/dev` et cumulent les commits, donc la décision réelle est FULL ; le
   comportement sélectif est prouvé par simulations du même `impact.mjs` et par la
   lecture de ses règles.

## 9. Reproduire

```powershell
# Revalidation locale (Node >= 24, binaire construit depuis HEAD)
cargo build -p ravum-todo
$env:RAVUM_TODO_BIN = "target\debug\ravum-todo.exe"
$env:RAVUM_E2E_REQUIRE_BROWSER = "1"
node apps/ravum-todo/e2e/browser_e2e.mjs `
  --screenshots=target\ravum\browser-e2e\screenshots `
  --report=target\ravum\browser-e2e\report.json
# exit 0 = 10/10 ; exit 2 = SKIP explicite (aucun navigateur) ; jamais de faux vert

# Analyse d'impact locale (chemins apps-only)
node tools/impact/impact.mjs --files "apps/ravum-todo/static/app.js" --json
node tools/impact/selftest.mjs   # sur un arbre propre (git archive) => PASS 52/505

# Preuves CI
gh run view 42538867282 --repo Rwanbt/Ravum --log --job=128909135716
gh run download 42538867282 --repo Rwanbt/Ravum -n browser-e2e-evidence -D <dir>
```

## 10. Prochaines étapes (à trancher par le Lead)

1. Décider et implémenter la règle §6 (impact.mjs + `mvp-impact.yml`) — **hors périmètre
   de cette mission**, qui ne devait modifier aucun workflow.
2. Si la règle est retenue, prévoir un run CI de validation sur un **PR purement
   apps-only** pour exercer le chemin sélectif + E2E et vérifier le coût réel.
3. Corriger le lanceur local `scripts/impact.ps1` (BOM UTF-8) ou le retirer de la doc de
   reproduction locale.
4. Étudier l'extension à un second moteur (Firefox/WebKit) et un premier niveau
   d'accessibilité si le produit grossit.
