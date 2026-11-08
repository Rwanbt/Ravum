# E2E navigateur réel — `ravum-todo`

Ce dossier contient un test **end-to-end dans un vrai navigateur** pour l'application
témoin `ravum-todo` : un Chromium/Edge headless est piloté via le **Chrome DevTools
Protocol (CDP)**, contre un **vrai serveur** `ravum-todo` (socket réelle, fichier SQLite
réel), et le test vérifie le **DOM réellement affiché** — pas seulement des codes HTTP.

Contrainte de conception : **zéro dépendance npm**, aucun `npm install`, aucun
`node_modules`. Node ≥ 24 (testé avec 26 ; la CI épingle 24) fournit `fetch`, `WebSocket`
et `node:sqlite` sans drapeau ; le pilote CDP tient en quelques centaines de lignes dans
`lib/`. (Node 22.5–23.3 exigerait `--experimental-sqlite` pour le seul scénario 9.)

## Démarrage rapide

```powershell
# Depuis la racine du dépôt
powershell -ExecutionPolicy Bypass -File scripts/demo-m4-browser.ps1
```

ou directement (Node ≥ 24 requis) :

```powershell
cargo build -p ravum-todo
$env:RAVUM_TODO_BIN = "target\debug\ravum-todo.exe"
node apps/ravum-todo/e2e/browser_e2e.mjs
```

Sans `RAVUM_TODO_BIN`, le harnais démarre le serveur avec
`cargo run -q -p ravum-todo -- --db … --port … --token …` (le script de démonstration et
la CI compilent une fois puis passent par la variable pour éviter la contention cargo).

## Découverte du navigateur

Ordre exact (le premier exécutable lisible gagne) :

1. `RAVUM_BROWSER` (chemin explicite ; une valeur invalide est une **erreur de
   configuration**, pas un skip) ;
2. headless shells installés par Playwright sous
   `%LOCALAPPDATA%\ms-playwright\chromium_headless_shell-*`, **révision la plus récente
   d'abord**, en essayant les deux dispositions connues :
   - `chrome-headless-shell-win64\chrome-headless-shell.exe` (disposition actuelle, celle
     de cette machine) ;
   - `chrome-win\headless_shell.exe` (disposition historique citée par le mandat) ;
3. Chromium complet installé par Playwright (`chromium-*\chrome-win64\chrome.exe`) ;
4. Chrome système `C:\Program Files\Google\Chrome\Application\chrome.exe` puis
   `C:\Program Files (x86)\…` ;
5. Edge système `C:\Program Files\Microsoft\Edge\Application\msedge.exe` puis
   `C:\Program Files (x86)\…` ;
6. sous Linux : `chromium`, `chromium-browser`, `google-chrome`, `google-chrome-stable`,
   `microsoft-edge`, `microsoft-edge-stable` depuis le `PATH`, puis le cache Playwright
   (`~/.cache/ms-playwright`, shells et Chromium complet).

Si rien n'est trouvé : **SKIP explicite** (code de sortie `2`, message qui liste les
chemins essayés). Avec `--require-browser` ou `RAVUM_E2E_REQUIRE_BROWSER=1`, c'est un
**échec** (code `1`) : la CI ne peut pas produire de faux vert.

Lancement (drapeaux du mandat) :

```
--headless=new --remote-debugging-port=0 --no-first-run --no-default-browser-check
--user-data-dir=<profil temporaire> --disable-gpu --disable-dev-shm-usage about:blank
```

L'URL `ws://127.0.0.1:<port>/devtools/browser/...` est lue sur **stderr** ; le port est
choisi par l'OS (`--remote-debugging-port=0`). Le profile temporaire est supprimé et le
processus (et ses descendants) tué dans tous les cas, succès comme échec.
Variables optionnelles : `RAVUM_BROWSER_NO_SANDBOX=1` (ajoute `--no-sandbox`, utile dans
un conteneur Linux), `RAVUM_BROWSER_EXTRA_ARGS="..."`.

## Architecture

```
browser_e2e.mjs        scénarios (10) + runner + rapport JSON + codes de sortie
lib/cdp.mjs            client CDP minimal : {id, method, params}, événements, sessions
                       aplaties (Target.attachToTarget flatten=true), timeouts
lib/browser.mjs        découverte, lancement, lecture de l'URL DevTools, BrowserSession
lib/page.mjs           un onglet : Runtime.evaluate (awaitPromise/returnByValue),
                       helpers DOM (fill/click/waitFor…), collecteurs console/réseau,
                       Page.captureScreenshot, assertions « console propre »
lib/server.mjs         ports libres, démarrage/arrêt du vrai serveur (arbre de
                       processus), health check, fetch JSON, base v1 legacy
                       (node:sqlite), proxy local de réécriture de contrat
```

Flux d'un scénario :

1. un port libre est demandé à l'OS, le serveur `ravum-todo` démarre sur une base SQLite
   temporaire, `/health` est attendu par `fetch` ;
2. un onglet est créé (`Target.createTarget` + `Target.attachToTarget`), les domaines
   `Page`, `Runtime`, `Log` et `Network` sont activés pour cette session ;
3. `Page.navigate`, puis attente d'éléments/messages par évaluations répétées ;
4. les actions passent par le DOM (vrai `.click()`, valeurs d'input posées via le setter
   natif + événements `input`/`change`) ;
5. le test vérifie le DOM, et souvent la vérité terrain côté serveur (`GET /api/tasks`) ;
6. `Runtime.exceptionThrown`, `Runtime.consoleAPICalled` (type error), `Log.entryAdded`
   (niveau error), `Network.responseReceived` (HTTP ≥ 400) et `Network.loadingFailed`
   sont collectés et confrontés à une liste blanche **explicite par scénario** ;
7. serveur arrêté, onglet fermé, profil navigateur et répertoire temporaire nettoyés.

Les scénarios qui doivent produire une erreur (400, 401, réseau) déclarent exactement ce
qui est attendu ; tout le reste est un échec. Aucun `skip` silencieux.

## Scénarios

| # | Scénario | Ce qui est vérifié dans le DOM |
|---|----------|-------------------------------|
| 1 | Chargement de l'interface | `document.title`, `h1`, formulaire (jeton, titre required, échéance, bouton), liste vide, bandeau « Contrat v1 vérifié » (version lue dans le texte affiché) |
| 2 | Création | saisie + clic, la tâche apparaît dans `#tasks`, les boutons Modifier/Terminer/Supprimer existent, champ titre vidé, ligne réellement en base |
| 3 | Modification du titre | éditeur inline (bouton Modifier), nouvelle valeur saisie, DOM re-rendu avec le nouveau titre, serveur d'accord |
| 4 | Suppression | clic Supprimer, disparition de la ligne, « aucune tâche » affiché, 0 tâche en base |
| 5 | Validation | titre vide → message DOM « le titre est obligatoire » (bloqué avant requête) ; titre « espaces » → erreur serveur 400 affichée, aucune tâche créée |
| 6 | Autorisation | jeton invalide → 401 affiché dans `#error`, page toujours fonctionnelle (h1/bandeau/liste) ; jeton absent → garde cliente affichée ; aucune exception non capturée |
| 7 | Incompatibilité de contrat | un proxy local réécrit `contract_version` à 2 ; bandeau « Client incompatible : contrat serveur v2, client v1. Mutations désactivées. », bouton de création désactivé, tâche existante lisible, aucune mutation émise |
| 8 | Persistance | création via l'UI, arrêt du serveur, redémarrage sur la même base, rechargement de la page : la tâche est toujours dans le DOM |
| 9 | Migration v1 → v2 | base `user_version=1` créée hors serveur ; après démarrage, `/health` dit `schema_version=2`, la ligne v1 est visible dans le DOM, une nouvelle tâche avec échéance est créée et affichée, `due_date` persistée |
| 10 | Erreur réseau | serveur arrêté, clic Rafraîchir puis soumission → message d'erreur visible dans `#error`, page vivante, **zéro** `Runtime.exceptionThrown` ; seuls les échecs réseau `ERR_CONNECTION_REFUSED` sont whitelistés |

## Options et variables

```
node apps/ravum-todo/e2e/browser_e2e.mjs --only=1,3 --screenshots=<dir> --report=<fichier> --require-browser
```

| Variable | Effet |
|---|---|
| `RAVUM_BROWSER` | chemin explicite du navigateur |
| `RAVUM_TODO_BIN` | binaire `ravum-todo` précompilé (sinon `cargo run`) |
| `RAVUM_ROOT` | racine du dépôt (défaut : déduite du fichier) |
| `RAVUM_E2E_REQUIRE_BROWSER=1` | pas de navigateur ⇒ échec (code 1), jamais SKIP |
| `RAVUM_E2E_SCREENSHOTS` | répertoire des captures PNG (une par scénario/onglet) |
| `RAVUM_E2E_REPORT` | chemin du rapport JSON de preuve |
| `RAVUM_E2E_SERVER_TIMEOUT_MS` | budget de démarrage du serveur en mode `cargo run` (180000 par défaut) |
| `RAVUM_TODO_HTTP_DEBUG=1` | active les traces HTTP brutes côté serveur (diagnostic) |

## Codes de sortie

| Code | Signification |
|---|---|
| `0` | les 10 scénarios ont passé (ou ceux demandés par `--only`) |
| `1` | au moins un scénario a échoué, ou erreur de harnais (browser/nav) |
| `2` | **SKIP explicite** : aucun navigateur trouvé, le JavaScript de l'UI n'a pas été validé |

La CI (`mvp.yml`) appelle `scripts/demo-m4-browser.ps1 -RequireBrowser -Screenshots` :
aucun navigateur = échec ; le rapport JSON et les captures sont publiés en artefact
`browser-e2e-evidence` (conservés 7 jours), y compris si l'étape échoue.

## Preuves et limites

Le rapport JSON (`report.json`) contient : OS, Node, `git` (branche/commit), produit et
révision du navigateur, chemin et source de découverte, drapeaux de lancement, mode du
serveur, et pour chaque scénario : statut, durée, étapes décrites, console/erreurs
collectées, journaux du serveur, captures.

Ce harnais **ne couvre pas** : accessibilité, TLS/HTTPS (le serveur de démonstration ne
l'implémente pas), multi-utilisateur concurrent, navigateurs non testés (Firefox/WebKit),
mobile, régressions visuelles, performance, ni une qualification de production. Le
document de mission `docs/mvp/MVP-0b-browser-e2e.md` liste les résultats réels et les
limites honnêtes.

## Défauts réels trouvés par ce harnais (MVP-0b)

1. **Suppression impossible dans l'UI** : le client n'envoyait `Authorization` que pour les
   requêtes avec corps ; `DELETE` (sans corps) répondait toujours `401`. Les tests HTTP
   ne pouvaient pas le voir. Corrigé dans `static/app.js` (`api()` attache le jeton à
   toute mutation).
2. **Serveur : 400 intermittents avec un vrai navigateur** : sous Windows, un socket
   accepté d'un listener non bloquant hérite du mode non bloquant ; la lecture de la
   requête pouvait renvoyer `WSAEWOULDBLOCK` avant l'arrivée des octets et produire un
   `400 malformed_request` aléatoire. Les tests Rust écrivent immédiatement après
   `connect` et ne voyaient jamais la course. Corrigé dans `src/routes.rs`
   (`stream.set_nonblocking(false)`).
3. **204 non conforme** : `Content-Length: 0` était envoyé avec un `204 No Content`
   (interdit par RFC 7230 §3.3.2). Corrigé dans `src/http.rs`.

Chromium 156 signale toutefois la requête `DELETE` répondant `204` comme `canceled`
(`net::ERR_ABORTED`) après réception de la réponse : c'est un artefact de comptabilité
DevTools, pas un échec réseau. Le harnais le traite comme tel (uniquement
`canceled=true` **et** réponse déjà reçue) et l'inscrit dans les étapes du scénario 4 ;
le DOM et la base restent les autorités.
