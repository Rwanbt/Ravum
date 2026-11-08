# MVP-0b / L4-A — E2E navigateur réel (CDP, zéro dépendance npm)

> **Portée** : le MVP-0a validait l'application témoin `ravum-todo` par des tests HTTP et
> SQLite (10 tests E2E Rust). Le JavaScript de l'interface n'était **jamais exécuté** : rien
> ne prouvait que le DOM affiché était correct, ni que les erreurs (400, 401, réseau,
> contrat incompatible) étaient visibles pour l'utilisateur. Ce document décrit le harnais
> navigateur ajouté, les résultats réellement obtenus sur cette machine, les défauts réels
> qu'il a révélés et les limites qui restent.
>
> **Statut des preuves** : `TESTED` en local (Windows 11, Chromium 156 headless) ;
> **`PENDING` en CI** (l'étape existe dans `mvp.yml` mais n'a pas encore tourné sur
> GitHub Actions). Aucun résultat n'est `PROVED`, aucune qualification navigateur large
> n'est revendiquée.
>
> Machine : Windows 11 Pro 26100, i7-13620H (10 cœurs / 16 threads), 15,71 Go RAM,
> `rustc 1.95.0` / `cargo 1.95.0`, Node `v26.13.0`. Campagne faite pendant que d'autres
> agents compilaient (charge partagée, voir §5 stabilité).

## 1. Problème exact

Les tests HTTP du MVP-0a appelaient les routes avec des sockets Rust ; ils ne pouvaient pas
voir :

- une erreur JavaScript qui casse le rendu (page blanche, exception non capturée) ;
- un contrat déclaré compatible mais un DOM qui ne reflète pas les données ;
- les messages d'erreur affichés (validation, autorisation, contrat, réseau) ;
- les défauts du client qui n'existent **que** dans le navigateur (en-têtes HTTP, gestion
  d'une connexion fermée par le serveur).

Exemple concret trouvé : la suppression était impossible dans l'UI (le client n'envoyait
le jeton que pour les requêtes avec corps ; `DELETE` répondait toujours `401`), alors que
tous les tests HTTP passaient. C'est exactement le trou que ce ticket devait fermer.

## 2. Harnais

Fichiers livrés :

| Fichier | Rôle |
|---|---|
| `apps/ravum-todo/e2e/browser_e2e.mjs` | 10 scénarios, runner, rapport JSON, codes de sortie |
| `apps/ravum-todo/e2e/lib/cdp.mjs` | client CDP minimal sur `WebSocket` natif : `{id, method, params}`, événements, timeouts, sessions aplaties (`Target.attachToTarget {flatten:true}`) |
| `apps/ravum-todo/e2e/lib/browser.mjs` | découverte du navigateur, lancement, lecture de l'URL `ws://…` sur stderr, `BrowserSession`, arrêt + nettoyage du profil |
| `apps/ravum-todo/e2e/lib/page.mjs` | un onglet : `Runtime.evaluate` (`awaitPromise`/`returnByValue`), helpers DOM (`fill`, `click`, `waitFor`…), collecteurs `Runtime.exceptionThrown`, `Runtime.consoleAPICalled`, `Log.entryAdded`, `Network.responseReceived`, `Network.loadingFailed`, `Page.captureScreenshot`, assertion « console propre » |
| `apps/ravum-todo/e2e/lib/server.mjs` | port libre, démarrage/arrêt du **vrai** serveur (arbre de processus), health check, `fetch` JSON, base v1 legacy (`node:sqlite`), proxy local de réécriture de contrat |
| `apps/ravum-todo/e2e/README.md` | mode d'emploi, découverte, options, codes de sortie |
| `scripts/demo-m4-browser.ps1` | lancement reproductible (Node ≥ 24, `cargo build -p ravum-todo`, code de sortie propagé) |
| `.github/workflows/mvp.yml` | étape CI + artefact de preuve |

Zéro dépendance npm : Node ≥ 24 (testé avec Node 26 ; Node 22.5–23.3 exigerait
`--experimental-sqlite` pour le seul scénario 9) fournit `fetch`, `WebSocket` et
`node:sqlite`. Aucun `npm install`, aucun `node_modules`.

Déroulé d'un scénario :

1. l'OS attribue un port libre ; `ravum-todo` démarre sur un fichier SQLite temporaire
   (`RAVUM_TODO_BIN` ou `cargo run -q -p ravum-todo --`) ; `/health` est attendu par
   `fetch` ;
2. un onglet est créé (`Target.createTarget` + `Target.attachToTarget`), les domaines
   `Page`, `Runtime`, `Log`, `Network` sont activés pour cette session ;
3. `Page.navigate` puis attente d'éléments/messages par évaluations répétées dans le
   contexte de la page ;
4. les actions sont réelles : `.click()` sur les boutons, valeurs d'input posées via le
   setter natif + événements `input`/`change` ;
5. le DOM est vérifié, puis souvent la vérité terrain côté serveur (`GET /api/tasks`) ;
6. le serveur est arrêté, l'onglet fermé, le profil navigateur et le répertoire temporaire
   supprimés — succès comme échec.

Le scénario 7 n'ajoute aucun drapeau de test au serveur : un petit proxy HTTP local
réécrit uniquement `contract_version` dans `/api/schema`. Le scénario 9 crée une base
`user_version=1` hors serveur avec `node:sqlite` (table sans `due_date`).

## 3. Scénarios et résultats réels

Commande exécutée (dernière campagne, avec captures) :

```powershell
powershell -ExecutionPolicy Bypass -File scripts/demo-m4-browser.ps1 -RequireBrowser -Screenshots
```

Navigateur réellement utilisé (le premier détecté, conformément à l'ordre documenté) :
`%LOCALAPPDATA%\ms-playwright\chromium_headless_shell-1248\chrome-headless-shell-win64\chrome-headless-shell.exe`
→ `HeadlessChrome/156.0.8078.4`, protocol CDP `1.3`, révision
`@f842f52ef39fd97b449eb27cff414bf05a98ddce`. Lancement avec
`--headless=new --remote-debugging-port=0 --no-first-run --no-default-browser-check
--user-data-dir=<profil temp> --disable-gpu --disable-dev-shm-usage about:blank` ; URL
DevTools lue sur stderr.

| # | Scénario | Vérifié dans le DOM (résumé) | Résultat | Durée |
|---|----------|------------------------------|----------|-------|
| 1 | Chargement de l'interface | `title` = « Ravum TODO — application témoin M4 », `h1`, formulaire complet (jeton, titre `required`, échéance, bouton), liste « aucune tâche », bandeau « Contrat v1 vérifié. » (v1 lu dans le texte) | **PASS** | 546 ms |
| 2 | Création | saisie + clic, la tâche apparaît dans `#tasks`, boutons Modifier/Terminer/Supprimer, champ vidé, 1 ligne en base | **PASS** | 497 ms |
| 3 | Modification du titre | éditeur inline (bouton Modifier), valeur initiale « titre initial », saisie puis Enregistrer, DOM re-rendu avec « titre modifié par le navigateur », serveur d'accord | **PASS** | 923 ms |
| 4 | Suppression | clic Supprimer, ligne absente du DOM, « aucune tâche » affiché, 0 tâche en base | **PASS** | 619 ms |
| 5 | Validation | titre vide → message DOM « le titre est obligatoire » (bloqué avant requête) ; titre `"   "` → message serveur « the request is not valid: the title must not be empty » ; aucune tâche créée | **PASS** | 490 ms |
| 6 | Autorisation | jeton invalide → 401 affiché dans `#error` (« a valid bearer token is required »), page toujours fonctionnelle ; jeton absent → « saisissez un jeton pour modifier les tâches » ; 0 exception non capturée | **PASS** | 488 ms |
| 7 | Contrat incompatible | bandeau « Client incompatible : contrat serveur v2, client v1. Mutations désactivées. » (classe `warn` visible), bouton de création désactivé, tâche existante lisible, aucune mutation émise (1 tâche avant/après) | **PASS** | 776 ms |
| 8 | Persistance | création via l'UI (port 58647), arrêt du serveur, redémarrage sur la même base (port 50951), rechargement : la tâche est toujours dans le DOM et en base | **PASS** | 1172 ms |
| 9 | Migration v1 → v2 | base v1 créée hors serveur ; `/health` = `schema_version=2`, `contract_version=1` ; ligne « tâche v1 héritée » visible ; nouvelle tâche avec échéance `2026-12-01` créée et affichée ; `due_date` persistée ; ligne v1 conservée (`due_date` absente = NULL) | **PASS** | 498 ms |
| 10 | Erreur réseau | serveur arrêté, clic Rafraîchir → « Failed to fetch » dans `#error` ; soumission → même erreur ; page vivante ; **0** `Runtime.exceptionThrown` ; seuls deux `ERR_CONNECTION_REFUSED` whitelistés | **PASS** | 1375 ms |

Total de la campagne de référence : **10/10 PASS, 0 échec, 0 skip, 44 vérifications
décrites, 12,7 s** (`durationMs=12668`). Le rapport JSON
`target/ravum/browser-e2e/report.json` est régénéré à chaque exécution ; les durées
varient de 10,0 à 13,1 s selon la charge machine (d'autres agents compilaient pendant les
campagnes).

Captures (locales, `target/` n'est pas versionné) :

```
target/ravum/browser-e2e/screenshots/01-…-s01.png  …  10-…-s10.png
```

Les scénarios 1–4 et 8–9 exigent une console **propre** (0 exception, 0 `console.error`,
0 erreur `Log`, 0 HTTP ≥ 400, 0 échec réseau) ; le scénario 4 documente une exception
Chromium de comptabilité (voir §4.3). Les scénarios 5, 6 et 10 déclarent explicitement les
seules erreurs attendues (HTTP 400, HTTP 401, `ERR_CONNECTION_REFUSED`) ; tout le reste
échoue.

## 4. Défauts réels révélés, corrections

### 4.1 Suppression impossible dans l'UI (bug client)

`static/app.js` : `api()` n'attachait `Authorization` que lorsque la requête avait un
corps. `DELETE /api/tasks/{id}` (sans corps) partait donc sans jeton → **401 systématique**,
alors que `PATCH` (avec corps) fonctionnait. Les tests HTTP ne pouvaient pas le voir.
Correction : toute méthode autre que `GET` attache le jeton (`api()`), et le scénario 4
vérifie la disparition dans le DOM **et** l'état de la base.

### 4.2 Serveur : 400 intermittents avec un vrai navigateur (bug serveur Windows)

Des `GET /` répondaient parfois `400 malformed_request` (~1 fois sur 8 chargements) alors
que le client n'avait rien envoyé d'invalide. Cause trouvée en traçant les octets bruts
côté serveur : sous Windows, un socket accepté depuis un listener **non bloquant** hérite
du mode non bloquant. `read_line` pouvait renvoyer `WSAEWOULDBLOCK` (os error 10035) avant
l'arrivée des octets de la requête ; traité comme une requête malformée, le serveur
répondait 400. Les tests Rust écrivent immédiatement après `connect` et ne tombent jamais
dans cette fenêtre ; un navigateur réel, lui, envoie sa requête quelques centaines de
microsecondes après le handshake.

Correction (`apps/ravum-todo/src/routes.rs`) : `stream.set_nonblocking(false)` au début de
chaque connexion. Validation : 12 chargements consécutifs du scénario 1 sans échec, puis
plusieurs campagnes complètes 10/10 (avant : ~1 échec sur 8).

Un mode diagnostic a été ajouté : `RAVUM_TODO_HTTP_DEBUG=1` trace les octets bruts et
l'erreur de parsing ; désactivé par défaut, il a servi à établir la cause et reste
disponible pour de futurs diagnostics.

### 4.3 204 non conforme (RFC 7230 §3.3.2) + artefact Chromium

Le serveur envoyait `Content-Length: 0` dans les réponses `204 No Content`, ce que la RFC
interdit. Correction (`src/http.rs`) : un 204 est écrit sans en-têtes d'entité.

Chromium 156 signale malgré tout la requête `DELETE` répondant `204` comme `canceled`
(`net::ERR_ABORTED`) **après** réception de la réponse (statut 204 observé, JS en succès,
DOM/base corrects). C'est un artefact de comptabilité DevTools, reproductible même sans
requête suivante. Le harnais ne l'ignore que pour le cas précis `canceled=true` **et**
réponse déjà reçue ; l'événement est inscrit dans les étapes du scénario 4 et le DOM reste
l'autorité. Le scénario ne masque donc aucun autre échec réseau.

### 4.4 Autres ajustements (dans le périmètre `apps/ravum-todo/**`)

- `static/app.js` : ajout de l'édition inline du titre (bouton « Modifier » → input +
  Enregistrer/Annuler), nécessaire au scénario 3 (l'API `PATCH {title}` existait déjà) ;
  message DOM pour un titre vide (`invalid` sur le champ `required`).
- `static/index.html` : favicon `data:,` pour éviter un `/favicon.ico` 404 parasite dans la
  console ; style de l'éditeur inline.

## 5. Stabilité des campagnes

Toutes ces exécutions sont sur la même machine, pendant que d'autres agents compilaient :

- après la correction 4.2 et les whitelists : **10/10** sur la campagne de référence,
  puis 4/4 campagnes complètes consécutives 10/10, puis 3/3 campagnes de stabilité
  finales 10/10 (durées 10,0–11,3 s hors captures) ;
- l'échec `400` aléatoire n'est plus réapparu sur les 15+ chargements de contrôle ;
- deux contre-expériences ont été faites : sans la correction `set_nonblocking(false)`,
  le scénario 1 échoue de façon intermittente ; avec elle, 12/12.

Le nettoyage est vérifié : aucun processus `ravum-todo`/`chrome-headless-shell` résiduel
après les campagnes, aucun répertoire temporaire de profil conservé (hors
`target/ravum/browser-e2e` pour les captures/rapport).

## 6. CI

Ajout dans `.github/workflows/mvp.yml` après la démo M4 :

- `actions/setup-node@v4` avec Node 24 (garantit `WebSocket`/`fetch` natifs) ;
- `scripts/demo-m4-browser.ps1 -RequireBrowser -Screenshots` : sur `windows-latest`, Edge
  est présent ; si aucun navigateur n'est détecté, l'étape **échoue** avec un message
  explicite (pas de faux vert) ;
- publication de l'artefact `browser-e2e-evidence` (rapport JSON + captures) même en cas
  d'échec.

Statut : étape **non encore exécutée en CI** (`PENDING`). Localement, les trois codes de
sortie ont été vérifiés : `0` succès, `1` échec explicite (`--require-browser` sans
navigateur), `2` SKIP explicite sans navigateur (message listant les chemins essayés).

## 7. Limites honnêtes (ce que ce harnais ne couvre pas)

- **Accessibilité** : aucun audit a11y (arbres d'accessibilité non assertés, pas de
  navigateur lecteur d'écran).
- **TLS/HTTPS** : le serveur de démonstration ne l'implémente pas ; le harnais ne teste que
  HTTP en clair sur loopback.
- **Multi-utilisateur / concurrence** : un seul onglet, une seule session, une seule
  connexion SQLite derrière un mutex ; aucune course multi-clients, aucun test de charge.
- **Navigateurs** : un seul moteur testé (Chromium 156 headless shell local). Firefox,
  WebKit/Safari, Chrome/Edge complets, mobile et vieilles versions ne sont **pas** testés.
  La CI utilisera Edge (moteur Chromium) ; le comportement Edge n'est pas encore établi.
- **Rendu visuel** : les captures sont des preuves d'exécution, pas une baseline de
  régression visuelle (pas de comparaison pixel, pas de CSS/layout asserté).
- **Performance** : aucune mesure de temps de chargement, de Web Vitals ou de budget.
- **Réseau réel** : pas de proxy HTTPS, pas de latence/coupure partielle (seule la coupure
  totale du serveur est testée), pas de TLS, pas de DNS.
- **Sécurité** : le jeton est saisi dans un champ mot de passe non persisté ; aucune
  vérification CSP, XSS, clickjacking.
- **Arrêt du serveur** : le redémarrage du scénario 8 tue le processus (pas d'arrêt
  gracieux ni de checkpoint WAL explicite) ; SQLite récupère le WAL, mais ce n'est pas un
  test de durabilité en cas de crash.
- **État des preuves** : `TESTED` local uniquement, `PENDING` CI. Ce document ne prouve pas
  que l'application est prête pour la production ; c'est une validation de la boucle
  UI→API→SQLite dans un navigateur réel.

## 8. Reproduire

```powershell
# Local (Windows, Chromium/Edge/Chrome ou headless shell Playwright)
powershell -ExecutionPolicy Bypass -File scripts/demo-m4-browser.ps1
# CI-like (aucun navigateur = échec explicite), avec captures et rapport
powershell -ExecutionPolicy Bypass -File scripts/demo-m4-browser.ps1 -RequireBrowser -Screenshots
# Sans script : Node >= 24 requis
cargo build -p ravum-todo
$env:RAVUM_TODO_BIN = "target\debug\ravum-todo.exe"
node apps/ravum-todo/e2e/browser_e2e.mjs --screenshots=target\ravum\browser-e2e\screenshots --report=target\ravum\browser-e2e\report.json
```

Tests de non-régression exécutés après les changements :

```
cargo test -p ravum-todo              → 4 tests unitaires + 10 tests E2E HTTP : OK
powershell -File scripts/demo-m4.ps1  → SUCCÈS (parcours HTTP + migration)
```

Aucun test existant n'a été supprimé, désactivé ou rendu silencieusement optionnel.

## 9. Prochaines étapes

1. Faire tourner l'étape CI sur `windows-latest` (Edge) et rattacher le rapport JSON comme
   preuve (`PENDING` → `TESTED` CI).
2. Étendre la découverte/validation à Firefox (CDP n'existe pas ; nécessiterait un autre
   protocole ou un ajout de dépendance, donc hors contrainte « zéro npm » actuelle).
3. Ajouter un audit a11y minimal (rôles/labels) et une baseline visuelle si le produit
   grossit.
4. Tester la concurrence multi-onglets/multi-clients sur le même serveur.
5. Décider si la démo `ravum-todo` doit rester un serveur « une requête par connexion » ou
   évoluer (keep-alive, TLS) — les défauts 4.2/4.3 montrent que ce genre d'écart ne se voit
   qu'avec un vrai client.
