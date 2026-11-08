#!/usr/bin/env node
// Ravum TODO — end-to-end browser test: a real headless Chromium/Edge, driven over
// the Chrome DevTools Protocol, against a real `ravum-todo` server (real socket, real
// SQLite file), asserting on the DOM that is actually displayed.
//
// Zero npm dependency: Node >= 24 provides `fetch`, `WebSocket` and `node:sqlite`
// without a flag (Node 22.5–23.3 needs --experimental-sqlite, not used here).
//
// Usage:
//   node apps/ravum-todo/e2e/browser_e2e.mjs [options]
//
// Options / environment:
//   --only=1,3              run only the listed scenario ids
//   --screenshots=<dir>     (or RAVUM_E2E_SCREENSHOTS) save one PNG per scenario
//   --report=<file>         (or RAVUM_E2E_REPORT) write the JSON evidence report
//   --require-browser       (or RAVUM_E2E_REQUIRE_BROWSER=1) no browser => failure
//   RAVUM_BROWSER           explicit browser executable path
//   RAVUM_TODO_BIN          explicit prebuilt ravum-todo executable (else cargo run)
//   RAVUM_E2E_SERVER_TIMEOUT_MS  startup budget when using cargo run (default 180000)
//
// Exit codes: 0 all scenarios passed, 1 at least one failed (or harness error),
// 2 explicit SKIP because no browser was found (never a silent green).

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import os from 'node:os';
import path from 'node:path';
import { discoverBrowser, launchBrowser } from './lib/browser.mjs';
import {
  createLegacyV1Database,
  fetchJson,
  resolveRepoRoot,
  rmWithRetry,
  sleep,
  startContractProxy,
  startTodoServer,
  stopTodoServer,
} from './lib/server.mjs';

const EXIT = { pass: 0, fail: 1, skip: 2 };

// ---------------------------------------------------------------------------
// CLI and runtime checks
// ---------------------------------------------------------------------------

function parseCli(argv) {
  const options = {
    only: null,
    screenshots: process.env.RAVUM_E2E_SCREENSHOTS ?? null,
    report: process.env.RAVUM_E2E_REPORT ?? null,
    requireBrowser: process.env.RAVUM_E2E_REQUIRE_BROWSER === '1',
    help: false,
  };
  for (const argument of argv) {
    if (argument === '--help' || argument === '-h') {
      options.help = true;
    } else if (argument.startsWith('--only=')) {
      const ids = argument
        .slice('--only='.length)
        .split(',')
        .map((value) => value.trim())
        .filter(Boolean)
        .map((value) => Number(value));
      if (!ids.length || ids.some((id) => !Number.isInteger(id))) {
        throw new Error(`--only attend une liste d'entiers (ex: --only=1,3), reçu: ${argument}`);
      }
      options.only = ids;
    } else if (argument.startsWith('--screenshots=')) {
      options.screenshots = argument.slice('--screenshots='.length);
    } else if (argument.startsWith('--report=')) {
      options.report = argument.slice('--report='.length);
    } else if (argument === '--require-browser') {
      options.requireBrowser = true;
    } else {
      throw new Error(`option inconnue: ${argument} (essayez --help)`);
    }
  }
  return options;
}

function printHelp() {
  console.log(`Ravum TODO — E2E navigateur réel (CDP, zéro dépendance npm)

Usage: node apps/ravum-todo/e2e/browser_e2e.mjs [options]

  --only=1,3            n'exécuter que les scénarios listés
  --screenshots=<dir>   enregistrer une capture PNG par scénario/page
  --report=<fichier>    écrire le rapport JSON de preuve
  --require-browser     échouer (code 1) si aucun navigateur n'est trouvé
  --help                afficher cette aide

Variables d'environnement: RAVUM_BROWSER, RAVUM_TODO_BIN, RAVUM_ROOT,
RAVUM_E2E_REQUIRE_BROWSER=1, RAVUM_E2E_SCREENSHOTS, RAVUM_E2E_REPORT,
RAVUM_E2E_SERVER_TIMEOUT_MS, RAVUM_BROWSER_NO_SANDBOX=1, RAVUM_BROWSER_EXTRA_ARGS.

Codes de sortie: 0 = succès, 1 = échec réel, 2 = SKIP explicite (aucun navigateur).`);
}

async function ensureRuntime() {
  const problems = [];
  const major = Number(process.versions.node.split('.')[0]);
  if (!Number.isInteger(major) || major < 22) {
    problems.push(`Node ${process.versions.node} < 22`);
  }
  if (typeof fetch !== 'function') problems.push('fetch global absent');
  if (typeof WebSocket !== 'function') problems.push('WebSocket global absent');
  try {
    // Node >= 23.4 exposes node:sqlite without a flag (used only to build the v1
    // database of scenario 9); Node 22.5–23.3 needs --experimental-sqlite.
    await import('node:sqlite');
  } catch (error) {
    problems.push(`node:sqlite indisponible (${error?.code ?? error?.message ?? 'module absent'})`);
  }
  if (problems.length) {
    throw new Error(
      `environnement Node incompatible avec le harnais: ${problems.join(', ')}. ` +
        `Node >= 24 (testé avec 26) est requis : WebSocket/fetch natifs et node:sqlite ` +
        `sans drapeau ; aucun paquet npm n'est installé.`,
    );
  }
}

// ---------------------------------------------------------------------------
// Scenario context: real servers, real tabs, deterministic cleanup
// ---------------------------------------------------------------------------

class TestContext {
  constructor({ repoRoot, browser, tempRoot, token, screenshotsDir }) {
    this.repoRoot = repoRoot;
    this.browser = browser;
    this.tempRoot = tempRoot;
    this.token = token;
    this.screenshotsDir = screenshotsDir;
    this.resources = { pages: [], servers: new Set(), proxies: new Set() };
    this.cleanupErrors = [];
  }

  beginScenario() {
    this.resources = { pages: [], servers: new Set(), proxies: new Set() };
    this.serverHistory = [];
  }

  async startServer({ name, dbName = 'todo.db', dbPath, label } = {}) {
    const directory = path.join(this.tempRoot, name ?? 'server');
    const resolvedDbPath = dbPath ?? path.join(directory, dbName);
    const server = await startTodoServer({
      root: this.repoRoot,
      dbPath: resolvedDbPath,
      token: this.token,
      label: label ?? name ?? 'ravum-todo',
    });
    this.resources.servers.add(server);
    this.serverHistory.push(server);
    return server;
  }

  async stopServer(server) {
    if (!server) return;
    this.resources.servers.delete(server);
    await stopTodoServer(server);
  }

  async waitServerDown(server, { timeoutMs = 5000 } = {}) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      try {
        await fetch(`${server.baseUrl}/health`, {
          headers: { Connection: 'close' },
          signal: AbortSignal.timeout(500),
        });
      } catch {
        return;
      }
      await sleep(100);
    }
    throw new Error(`le serveur "${server.label}" répond encore après son arrêt`);
  }

  async newPage(url, { waitReady = true, label = 'page' } = {}) {
    const page = await this.browser.newPage({ url, label });
    this.resources.pages.push(page);
    if (waitReady) await page.waitForBanner();
    return page;
  }

  async closePage(page) {
    const index = this.resources.pages.indexOf(page);
    if (index >= 0) this.resources.pages.splice(index, 1);
    await page.close();
  }

  async startProxy(options) {
    const proxy = await startContractProxy(options);
    this.resources.proxies.add(proxy);
    return proxy;
  }

  async stopProxy(proxy) {
    if (!proxy) return;
    this.resources.proxies.delete(proxy);
    await proxy.close();
  }

  /** Ground truth from the real server, next to the DOM assertions. */
  async tasks(server) {
    const reply = await fetchJson(`${server.baseUrl}/api/tasks`);
    assert.equal(reply.status, 200, `GET /api/tasks a répondu HTTP ${reply.status}`);
    return reply.payload.tasks;
  }

  async createTask(server, { title, due_date }) {
    const reply = await fetchJson(`${server.baseUrl}/api/tasks`, {
      method: 'POST',
      token: this.token,
      body: { title, due_date },
    });
    assert.equal(
      reply.status,
      201,
      `POST /api/tasks a répondu HTTP ${reply.status}: ${JSON.stringify(reply.payload)}`,
    );
    return reply.payload.task;
  }

  collectScenarioConsole() {
    return this.resources.pages.map((page) => page.snapshot());
  }

  async endScenario() {
    for (const page of this.resources.pages.splice(0)) {
      try {
        await page.close();
      } catch (error) {
        this.cleanupErrors.push(`fermeture page ${page.label}: ${error.message}`);
      }
    }
    for (const proxy of this.resources.proxies) {
      try {
        await proxy.close();
      } catch (error) {
        this.cleanupErrors.push(`fermeture proxy: ${error.message}`);
      }
    }
    this.resources.proxies.clear();
    for (const server of this.resources.servers) {
      try {
        await stopTodoServer(server);
      } catch (error) {
        this.cleanupErrors.push(`arrêt serveur ${server.label}: ${error.message}`);
      }
    }
    this.resources.servers.clear();
  }

  async dispose() {
    await this.endScenario();
  }
}

// ---------------------------------------------------------------------------
// Shared scenario helpers
// ---------------------------------------------------------------------------

async function createTaskViaUi(page, token, title, { due } = {}) {
  await page.fill('#token', token);
  await page.fill('#title', title);
  if (due !== undefined) await page.fill('#due', due);
  await page.click('#new-task button[type="submit"]');
  await page.waitForTask(title);
}

function slugify(text) {
  return text
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/[^a-zA-Z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .toLowerCase()
    .slice(0, 64);
}

// ---------------------------------------------------------------------------
// Scenarios (one per mandated check; each asserts on the displayed DOM)
// ---------------------------------------------------------------------------

const SCENARIOS = [
  {
    id: 1,
    name: "chargement de l'interface (titre, formulaire, version de contrat)",
    async run(ctx, step) {
      const server = await ctx.startServer({ name: 's01-load' });
      const page = await ctx.newPage(server.baseUrl, { waitReady: false, label: 's01' });

      await page.waitForElement('#new-task');
      const documentTitle = await page.evaluate('document.title');
      assert.match(documentTitle, /Ravum TODO/, `titre de page inattendu: ${documentTitle}`);
      step(`titre de la page: ${JSON.stringify(documentTitle)}`);

      const heading = await page.text('h1');
      assert.equal(heading?.trim(), 'Ravum TODO', `h1 inattendu: ${JSON.stringify(heading)}`);
      step(`h1 affiché: ${JSON.stringify(heading.trim())}`);

      const bannerText = await page.waitForBanner();
      assert.match(
        bannerText,
        /Contrat v1 vérifié/,
        `l'UI n'affiche pas le contrat v1: ${JSON.stringify(bannerText)}`,
      );
      const displayedVersion = /Contrat v(\d+) vérifié/.exec(bannerText)?.[1];
      assert.equal(displayedVersion, '1', `version de contrat affichée inattendue: ${bannerText}`);
      step(`bandeau contrat affiché: ${JSON.stringify(bannerText)}`);

      // The form itself: token field, title (required), due date, submit button.
      const formShape = await page.evaluate(`(() => ({
        hasForm: Boolean(document.querySelector('#new-task')),
        tokenField: Boolean(document.querySelector('#token')),
        titleField: Boolean(document.querySelector('#title')),
        titleRequired: document.querySelector('#title')?.required ?? false,
        dueField: Boolean(document.querySelector('#due')),
        submitButtons: [...document.querySelectorAll('#new-task button')].map((b) => b.textContent.trim()),
        listRendered: Boolean(document.querySelector('#tasks')),
      }))()`);
      assert.equal(formShape.hasForm, true, 'formulaire de création absent du DOM');
      assert.equal(formShape.tokenField, true, 'champ jeton absent du DOM');
      assert.equal(formShape.titleField, true, 'champ titre absent du DOM');
      assert.equal(formShape.titleRequired, true, 'le champ titre devrait être required');
      assert.equal(formShape.dueField, true, 'champ échéance absent du DOM');
      assert.deepEqual(formShape.submitButtons, ['Ajouter'], 'bouton de création inattendu');
      step(`formulaire présent: ${JSON.stringify(formShape)}`);

      await page.waitForText('#tasks', 'aucune tâche');
      step('liste vide rendue par le DOM ("aucune tâche")');

      const consoleState = page.assertCleanConsole();
      step(`console propre (aucune exception, aucune erreur): ${JSON.stringify(consoleState)}`);
    },
  },
  {
    id: 2,
    name: 'création d’une tâche et affichage dans le DOM',
    async run(ctx, step) {
      const server = await ctx.startServer({ name: 's02-create' });
      const page = await ctx.newPage(server.baseUrl, { label: 's02' });

      const title = 'tâche créée par le navigateur';
      await createTaskViaUi(page, ctx.token, title);
      step(`tâche visible dans le DOM: ${JSON.stringify(title)}`);

      const titles = await page.titles();
      assert.deepEqual(titles, [title], `liste du DOM inattendue: ${JSON.stringify(titles)}`);

      const items = await page.listItems();
      assert.equal(items.length, 1, `nombre d'éléments DOM inattendu: ${items.length}`);
      assert.equal(items[0].done, false, 'la nouvelle tâche ne devrait pas être terminée');
      assert.ok(items[0].buttons.includes('Modifier'), 'bouton Modifier absent du DOM');
      assert.ok(items[0].buttons.includes('Terminer'), 'bouton Terminer absent du DOM');
      assert.ok(items[0].buttons.includes('Supprimer'), 'bouton Supprimer absent du DOM');
      step(`élément DOM: ${JSON.stringify(items[0])}`);

      assert.equal(await page.errorText(), '', 'aucune erreur ne devrait être affichée');
      assert.equal(
        await page.evaluate(`document.querySelector('#title').value`),
        '',
        "le champ titre devrait être vidé après création",
      );

      // Ground truth next to the DOM: the row really exists in SQLite.
      const tasks = await ctx.tasks(server);
      assert.equal(tasks.length, 1, `le serveur ne contient pas 1 tâche: ${JSON.stringify(tasks)}`);
      assert.equal(tasks[0].title, title);

      const consoleState = page.assertCleanConsole();
      step(`console propre: ${JSON.stringify(consoleState)}`);
    },
  },
  {
    id: 3,
    name: 'modification du titre et mise à jour du DOM',
    async run(ctx, step) {
      const server = await ctx.startServer({ name: 's03-rename' });
      const page = await ctx.newPage(server.baseUrl, { label: 's03' });

      await createTaskViaUi(page, ctx.token, 'titre initial');
      await page.clickButtonWithText('#tasks li', 'Modifier');
      await page.waitForElement('.edit-title');
      const editedValue = await page.evaluate(`document.querySelector('.edit-title').value`);
      assert.equal(editedValue, 'titre initial', `valeur initiale de l'éditeur: ${editedValue}`);
      step(`éditeur inline ouvert avec la valeur ${JSON.stringify(editedValue)}`);

      const newTitle = 'titre modifié par le navigateur';
      await page.fill('.edit-title', newTitle);
      await page.clickButtonWithText('#tasks li', 'Enregistrer');
      await page.waitForTask(newTitle);
      const titles = await page.titles();
      assert.deepEqual(titles, [newTitle], `DOM après renommage: ${JSON.stringify(titles)}`);
      assert.ok(!titles.includes('titre initial'), "l'ancien titre est encore dans le DOM");
      step(`DOM mis à jour: ${JSON.stringify(titles)}`);

      const tasks = await ctx.tasks(server);
      assert.equal(tasks.length, 1, 'le serveur ne devrait avoir qu’une tâche');
      assert.equal(tasks[0].title, newTitle, 'le serveur n’a pas enregistré le nouveau titre');
      step(`serveur d'accord avec le DOM: title=${JSON.stringify(tasks[0].title)}`);

      const consoleState = page.assertCleanConsole();
      step(`console propre: ${JSON.stringify(consoleState)}`);
    },
  },
  {
    id: 4,
    name: 'suppression et disparition du DOM',
    async run(ctx, step) {
      const server = await ctx.startServer({ name: 's04-delete' });
      const page = await ctx.newPage(server.baseUrl, { label: 's04' });

      const title = 'tâche à supprimer';
      await createTaskViaUi(page, ctx.token, title);
      assert.deepEqual(await page.titles(), [title], 'la tâche créée devrait être dans le DOM');

      await page.clickButtonWithText('#tasks li', 'Supprimer');
      await page.waitForNoTask(title);
      await page.waitForText('#tasks', 'aucune tâche');
      const titles = await page.titles();
      assert.deepEqual(titles, [], `DOM après suppression: ${JSON.stringify(titles)}`);
      assert.match((await page.text('#tasks')) ?? '', /aucune tâche/);
      step('tâche absente du DOM et liste vide affichée ("aucune tâche")');

      const tasks = await ctx.tasks(server);
      assert.equal(tasks.length, 0, `le serveur contient encore ${tasks.length} tâche(s)`);
      step('serveur d’accord avec le DOM: 0 tâche');

      const consoleState = page.assertCleanConsole();
      step(
        'console propre: aucune exception, aucune erreur HTTP/log inattendue; ' +
          'EXPECTED: Chromium 156 marque la requête DELETE 204 comme "canceled" ' +
          `(ERR_ABORTED) après réception de la réponse — ${JSON.stringify(consoleState)}`,
      );
    },
  },
  {
    id: 5,
    name: 'erreur de validation (titre vide) affichée dans le DOM',
    async run(ctx, step) {
      const server = await ctx.startServer({ name: 's05-validation' });
      const page = await ctx.newPage(server.baseUrl, { label: 's05' });
      await page.fill('#token', ctx.token);

      // (a) truly empty title: the browser blocks submission, the DOM still shows why.
      await page.fill('#title', '');
      await page.click('#new-task button[type="submit"]');
      const emptyTitleError = await page.waitForErrorContaining('le titre est obligatoire');
      step(`titre vide → message DOM: ${JSON.stringify(emptyTitleError)}`);
      assert.equal((await ctx.tasks(server)).length, 0, 'une tâche a été créée depuis un titre vide');

      // (b) whitespace-only title: passes HTML `required`, the server rejects it, the UI
      // displays the server message (shared validation is the source of truth).
      await page.fill('#title', '   ');
      await page.click('#new-task button[type="submit"]');
      const serverValidationError = await page.waitForErrorContaining('title must not be empty');
      step(`titre espaces → erreur serveur affichée: ${JSON.stringify(serverValidationError)}`);
      assert.equal((await ctx.tasks(server)).length, 0, 'une tâche a été créée depuis "   "');

      assert.match((await page.text('#tasks')) ?? '', /aucune tâche/, 'la liste devrait rester vide');
      const consoleState = page.assertCleanConsole({
        allowHttpErrors: [/400 .*\/api\/tasks/],
        allowLogErrors: [/400 .*\/api\/tasks/],
      });
      step(`console/network attendus (HTTP 400 whitelisté): ${JSON.stringify(consoleState)}`);
    },
  },
  {
    id: 6,
    name: 'erreur d’autorisation (401) affichée sans casser l’UI',
    async run(ctx, step) {
      const server = await ctx.startServer({ name: 's06-auth' });
      const page = await ctx.newPage(server.baseUrl, { label: 's06' });
      const title = 'tâche refusée par le serveur';

      // (a) 401 from the real server: wrong token.
      await page.fill('#token', 'jeton-invalide');
      await page.fill('#title', title);
      await page.click('#new-task button[type="submit"]');
      const authError = await page.waitForErrorContaining('bearer token');
      step(`jeton invalide → erreur DOM: ${JSON.stringify(authError)}`);
      assert.equal((await ctx.tasks(server)).length, 0, 'la tâche a été créée malgré un 401');

      // (b) the page is still alive and usable (no blank page, no broken renderer).
      assert.match((await page.text('h1')) ?? '', /Ravum TODO/);
      await page.waitForText('#tasks', 'aucune tâche');
      assert.match((await page.text('#banner')) ?? '', /Contrat v1 vérifié/);
      step('page toujours fonctionnelle après 401 (h1, bandeau contrat, liste rendus)');

      // (c) missing token: the client refuses before the request, still in the DOM.
      await page.fill('#token', '');
      await page.fill('#title', 'tâche sans jeton');
      await page.click('#new-task button[type="submit"]');
      const missingTokenError = await page.waitForErrorContaining('saisissez un jeton');
      step(`jeton absent → garde cliente affichée: ${JSON.stringify(missingTokenError)}`);

      // No uncaught exception is tolerated; the 401 is the only expected noise.
      assert.equal(page.exceptions.length, 0, `exception non capturée: ${JSON.stringify(page.exceptions)}`);
      const consoleState = page.assertCleanConsole({
        allowHttpErrors: [/401 .*\/api\/tasks/],
        allowLogErrors: [/401 .*\/api\/tasks/],
      });
      step(`aucune exception non capturée; 401 whitelisté: ${JSON.stringify(consoleState)}`);
    },
  },
  {
    id: 7,
    name: 'incompatibilité de contrat (v2 côté serveur) → message UI explicite',
    async run(ctx, step) {
      const server = await ctx.startServer({ name: 's07-contract' });
      const visibleTask = 'tâche lisible malgré le contrat';
      await ctx.createTask(server, { title: visibleTask });

      // A tiny local proxy rewrites only /api/schema, so the client sees contract v2.
      const proxy = await ctx.startProxy({ upstreamBaseUrl: server.baseUrl, contractVersion: 2 });
      step(`proxy local ${proxy.baseUrl} → ${server.baseUrl}, contract_version forcé à 2`);

      const page = await ctx.newPage(proxy.baseUrl, { waitReady: false, label: 's07' });
      const bannerText = await page.waitForBanner({ compatible: false });
      assert.match(bannerText, /Client incompatible/i, `bandeau inattendu: ${JSON.stringify(bannerText)}`);
      assert.match(bannerText, /contrat serveur v2/i, `version serveur absente du message: ${bannerText}`);
      assert.match(bannerText, /client v1/i, `version cliente absente du message: ${bannerText}`);
      assert.match(bannerText, /Mutations désactivées/i, `désactivation absente du message: ${bannerText}`);
      step(`message d'incompatibilité affiché: ${JSON.stringify(bannerText)}`);

      // Not a blank page: the contract banner is shown, reads still render, and the
      // mutating control is disabled.
      await page.waitForTask(visibleTask);
      assert.equal(
        await page.evaluate(`getComputedStyle(document.querySelector('#banner')).display`),
        'block',
        'le bandeau doit être visible',
      );
      assert.equal(
        await page.isDisabled('#new-task button[type="submit"]'),
        true,
        'le bouton de création devrait être désactivé',
      );
      assert.match((await page.text('h1')) ?? '', /Ravum TODO/);
      step('page non blanche: bandeau visible, tâche lue affichée, bouton de mutation désactivé');

      // Clicking the disabled control must not create anything on the server.
      const before = (await ctx.tasks(server)).length;
      await page.click('#new-task button[type="submit"]');
      await sleep(400);
      const after = (await ctx.tasks(server)).length;
      assert.equal(after, before, `une mutation a eu lieu malgré le contrat incompatible (${before} → ${after})`);
      step(`aucune mutation émise (${before} tâche(s) avant et après)`);

      assert.equal(page.exceptions.length, 0, `exception non capturée: ${JSON.stringify(page.exceptions)}`);
      const consoleState = page.assertCleanConsole();
      step(`aucune exception non capturée; console propre: ${JSON.stringify(consoleState)}`);
      await ctx.stopProxy(proxy);
    },
  },
  {
    id: 8,
    name: 'persistance: redémarrage du serveur sur la même base et rechargement',
    async run(ctx, step) {
      const name = 's08-persistence';
      const dbPath = path.join(ctx.tempRoot, name, 'persist.db');
      const title = 'tâche persistante après redémarrage';

      const firstServer = await ctx.startServer({ name, dbPath, label: 's08-premier' });
      const firstPage = await ctx.newPage(firstServer.baseUrl, { label: 's08-page1' });
      await createTaskViaUi(firstPage, ctx.token, title);
      step(`créée sur le premier serveur (port ${firstServer.port}) et visible dans le DOM`);
      firstPage.assertCleanConsole();
      await ctx.closePage(firstPage);

      await ctx.stopServer(firstServer);
      await ctx.waitServerDown(firstServer);
      step('premier serveur arrêté (socket fermée)');

      const secondServer = await ctx.startServer({ name: `${name}-relance`, dbPath, label: 's08-second' });
      const secondPage = await ctx.newPage(secondServer.baseUrl, { label: 's08-page2' });
      await secondPage.waitForTask(title);
      const titles = await secondPage.titles();
      assert.deepEqual(titles, [title], `DOM après redémarrage: ${JSON.stringify(titles)}`);
      step(`après redémarrage (port ${secondServer.port}) et rechargement: ${JSON.stringify(titles)}`);

      const tasks = await ctx.tasks(secondServer);
      assert.equal(tasks.length, 1, `le serveur relancé ne contient pas la tâche: ${JSON.stringify(tasks)}`);
      assert.equal(tasks[0].title, title);
      step(`serveur relancé d'accord avec le DOM: ${JSON.stringify(tasks[0].title)}`);

      const consoleState = secondPage.assertCleanConsole();
      step(`console propre: ${JSON.stringify(consoleState)}`);
    },
  },
  {
    id: 9,
    name: 'migration v1 → v2: UI fonctionnelle et données v1 visibles',
    async run(ctx, step) {
      const name = 's09-migration';
      const dbPath = path.join(ctx.tempRoot, name, 'legacy-v1.db');
      await createLegacyV1Database(dbPath);
      step(`base v1 créée hors serveur: table sans due_date, 1 ligne, user_version=1 (${dbPath})`);

      const server = await ctx.startServer({ name, dbPath, label: 's09-v2' });
      assert.equal(
        server.health.schema_version,
        2,
        `le serveur aurait dû migrer en v2, /health dit ${server.health.schema_version}`,
      );
      assert.equal(server.health.contract_version, 1, 'contract_version inattendu');
      step(`/health: schema_version=${server.health.schema_version} contract_version=${server.health.contract_version}`);

      const page = await ctx.newPage(server.baseUrl, { label: 's09' });
      await page.waitForTask('tâche v1 héritée');
      step(`ligne v1 visible dans le DOM après migration`);

      // The migrated column is really usable through the UI.
      await createTaskViaUi(page, ctx.token, 'tâche post-migration', { due: '2026-12-01' });
      await page.waitForTask('tâche post-migration');
      const items = await page.listItems();
      const created = items.find((item) => item.title === 'tâche post-migration');
      assert.ok(created, `tâche post-migration absente du DOM: ${JSON.stringify(items)}`);
      assert.equal(created.due, '2026-12-01', `échéance non affichée dans le DOM: ${JSON.stringify(created)}`);
      step(`nouvelle tâche avec échéance affichée: ${JSON.stringify(created)}`);

      const tasks = await ctx.tasks(server);
      assert.equal(tasks.length, 2, `le serveur devrait contenir 2 tâches: ${JSON.stringify(tasks)}`);
      const migrated = tasks.find((task) => task.title === 'tâche v1 héritée');
      assert.ok(migrated, 'la ligne v1 a disparu côté serveur');
      // The shared contract serialises an absent date by *omitting* the field
      // (`skip_serializing_if`), so "NULL in SQLite" is `undefined` in JSON.
      assert.ok(
        migrated.due_date === null || migrated.due_date === undefined,
        `la ligne v1 devrait avoir due_date NULL: ${JSON.stringify(migrated)}`,
      );
      const written = tasks.find((task) => task.title === 'tâche post-migration');
      assert.equal(written.due_date, '2026-12-01', 'due_date non persistée après migration');
      step('serveur d’accord avec le DOM: ligne v1 conservée (due_date NULL), due_date v2 persistée');

      const consoleState = page.assertCleanConsole();
      step(`console propre: ${JSON.stringify(consoleState)}`);
    },
  },
  {
    id: 10,
    name: 'erreur réseau (serveur arrêté) visible, aucune exception non capturée',
    async run(ctx, step) {
      const server = await ctx.startServer({ name: 's10-network' });
      const page = await ctx.newPage(server.baseUrl, { label: 's10' });
      await createTaskViaUi(page, ctx.token, 'tâche avant la coupure');
      step('page chargée avec une tâche, puis serveur arrêté');

      await ctx.stopServer(server);
      await ctx.waitServerDown(server);
      step(`serveur arrêté (${server.baseUrl} ne répond plus)`);

      // Action 1: refresh an existing UI against the dead server.
      await page.click('#refresh');
      const refreshError = await page.waitFor(
        `(() => { const el = document.querySelector('#error');
                  return el && el.textContent.trim().length > 0 ? el.textContent : false; })()`,
        "un message d'erreur DOM après l'arrêt du serveur",
      );
      assert.match(
        refreshError,
        /fetch|network|réseau|connexion|refused/i,
        `message d'erreur réseau inattendu: ${JSON.stringify(refreshError)}`,
      );
      step(`rafraîchir hors ligne → erreur DOM: ${JSON.stringify(refreshError)}`);

      // Action 2: a mutation against the dead server must fail in the DOM too.
      await page.fill('#title', 'action hors ligne');
      await page.click('#new-task button[type="submit"]');
      await sleep(500);
      const mutationError = await page.errorText();
      assert.ok(mutationError.trim().length > 0, "aucune erreur affichée pour l'action hors ligne");
      assert.match(
        mutationError,
        /fetch|network|réseau|connexion|refused/i,
        `message d'erreur réseau inattendu: ${JSON.stringify(mutationError)}`,
      );
      step(`création hors ligne → erreur DOM: ${JSON.stringify(mutationError)}`);

      // The page must still be alive (not a blank page), and no uncaught exception is
      // allowed: network failures are caught by the client.
      assert.match((await page.text('h1')) ?? '', /Ravum TODO/);
      assert.equal(page.exceptions.length, 0, `exception non capturée: ${JSON.stringify(page.exceptions)}`);
      const consoleState = page.assertCleanConsole({
        allowNetworkFailures: [/ERR_CONNECTION_REFUSED/],
        allowLogErrors: [/Failed to load resource/],
      });
      step(`page vivante, aucune exception non capturée; échecs réseau attendus: ${JSON.stringify(consoleState)}`);
    },
  },
];

// ---------------------------------------------------------------------------
// Runner, diagnostics, report
// ---------------------------------------------------------------------------

function logScenarioDiagnostics(state) {
  console.error(`\n--- diagnostic scénario ${state.id} (${state.name}) ---`);
  console.error(state.error?.split('\n').slice(0, 12).join('\n') ?? 'aucune erreur enregistrée');
  for (const page of state.console ?? []) {
    console.error(
      `page "${page.label}": exceptions=${page.exceptions.length} consoleErrors=${page.consoleErrors.length} ` +
        `logErrors=${page.logErrors.length} httpErrors=${page.httpErrors.length} networkFailures=${page.networkFailures.length}`,
    );
    for (const entry of page.exceptions) {
      console.error(`  exception: ${entry.description ?? entry.text}`);
    }
    for (const entry of page.logErrors) console.error(`  log error: ${entry.text} (${entry.url ?? ''})`);
    for (const entry of page.consoleErrors) console.error(`  console.error: ${entry.text}`);
    for (const entry of page.httpErrors) console.error(`  http ${entry.status}: ${entry.url}`);
    for (const entry of page.networkFailures) {
      console.error(
        `  network: ${entry.errorText} ${entry.method ?? ''} ${entry.url} ` +
          `(response status: ${entry.responseStatus ?? 'aucune'}, canceled: ${entry.canceled}, ` +
          `blockedReason: ${entry.blockedReason ?? 'aucune'})`,
      );
    }
  }
  for (const server of state.serverLogs ?? []) {
    if (server.stderr.trim()) {
      console.error(`serveur "${server.label}" stderr: ${server.stderr.trim()}`);
    }
  }
}

async function runScenarios(ctx, options) {
  const selected = options.only
    ? SCENARIOS.filter((scenario) => options.only.includes(scenario.id))
    : SCENARIOS;
  if (!selected.length) {
    throw new Error(`--only=${options.only?.join(',')} ne sélectionne aucun scénario connu`);
  }
  const results = [];
  for (const scenario of selected) {
    ctx.beginScenario();
    const state = {
      id: scenario.id,
      name: scenario.name,
      status: 'PASS',
      steps: [],
      error: null,
      durationMs: 0,
      console: null,
      screenshots: [],
    };
    const step = (text) => state.steps.push(text);
    const startedAt = performance.now();
    console.log(`\n[${String(scenario.id).padStart(2, '0')}/${SCENARIOS.length}] ${scenario.name}`);
    try {
      await scenario.run(ctx, step);
    } catch (error) {
      state.status = 'FAIL';
      state.error = error?.stack ?? String(error);
    } finally {
      state.console = ctx.collectScenarioConsole();
      state.serverLogs = ctx.serverHistory.map((server) => ({
        label: server.label,
        mode: server.mode,
        command: server.logs.command,
        health: server.health ?? null,
        stdout: server.logs.stdout.slice(-2000),
        stderr: server.logs.stderr.slice(-4000),
      }));
      state.durationMs = Math.round(performance.now() - startedAt);
      if (ctx.screenshotsDir && state.console.length) {
        const slug = slugify(scenario.name);
        for (const page of ctx.resources.pages) {
          const file = path.join(
            ctx.screenshotsDir,
            `${String(scenario.id).padStart(2, '0')}-${slug}-${page.label}.png`,
          );
          try {
            state.screenshots.push(await page.screenshot(file));
          } catch (error) {
            state.cleanupWarning = `capture ${page.label}: ${error.message}`;
          }
        }
      }
      if (state.status === 'FAIL') logScenarioDiagnostics(state);
      await ctx.endScenario();
    }
    console.log(`     ${state.status} en ${state.durationMs} ms (${state.steps.length} vérifications décrites)`);
    results.push(state);
  }
  return results;
}

function readGit(repoRoot) {
  const run = (args) => {
    const result = spawnSync('git', args, { cwd: repoRoot, encoding: 'utf8', windowsHide: true });
    return result.status === 0 ? result.stdout.trim() : null;
  };
  return {
    branch: run(['rev-parse', '--abbrev-ref', 'HEAD']),
    commit: run(['rev-parse', 'HEAD']),
  };
}

function buildReport({
  repoRoot,
  discovery,
  browser,
  results,
  startedAt,
  finishedAt,
  harnessError,
  options,
}) {
  return {
    harness: 'apps/ravum-todo/e2e/browser_e2e.mjs',
    what: 'E2E navigateur réel (CDP) : DOM affiché, serveur réel (socket + SQLite), zéro dépendance npm',
    startedAt: startedAt.toISOString(),
    finishedAt: finishedAt.toISOString(),
    durationMs: finishedAt.getTime() - startedAt.getTime(),
    os: {
      platform: process.platform,
      release: os.release(),
      version: typeof os.version === 'function' ? os.version() : null,
      arch: process.arch,
      totalMemoryBytes: os.totalmem(),
    },
    node: process.version,
    git: readGit(repoRoot),
    browser: {
      product: browser?.version?.product ?? null,
      protocolVersion: browser?.version?.protocolVersion ?? null,
      revision: browser?.version?.revision ?? null,
      executablePath: discovery?.executablePath ?? null,
      source: discovery?.source ?? null,
      launchArguments: browser?.launchArguments ?? null,
    },
    server: {
      mode: process.env.RAVUM_TODO_BIN
        ? `RAVUM_TODO_BIN=${process.env.RAVUM_TODO_BIN} (binaire précompilé)`
        : 'cargo run -q -p ravum-todo --',
      token: '(aléatoire par exécution, non journalisé)',
    },
    options: { only: options.only, screenshots: options.screenshots, report: options.report },
    scenarios: results,
    totals: {
      scenarios: results.length,
      pass: results.filter((result) => result.status === 'PASS').length,
      fail: results.filter((result) => result.status === 'FAIL').length,
      documentedChecks: results.reduce((total, result) => total + result.steps.length, 0),
    },
    harnessError: harnessError ? String(harnessError.stack ?? harnessError) : null,
    caveat:
      'Environnement local unique; les preuves sont TESTED/PENDING, jamais PROVED. ' +
      "Aucun résultat CI n'est établi par ce rapport local.",
  };
}

function printSummary(results, { browser, discovery, durationMs, reportPath }) {
  console.log('\n=== Résumé ===');
  console.table(
    results.map((result) => ({
      scénario: result.id,
      statut: result.status,
      'durée (ms)': result.durationMs,
      'vérifications décrites': result.steps.length,
      nom: result.name,
    })),
  );
  const pass = results.filter((result) => result.status === 'PASS').length;
  const fail = results.length - pass;
  console.log(`Navigateur: ${browser?.version?.product ?? 'inconnu'} (${discovery?.executablePath ?? '?'})`);
  console.log(`Source: ${discovery?.source ?? '?'}`);
  console.log(
    `Résultat: ${pass}/${results.length} scénarios réussis, ${fail} échec(s), durée ${(durationMs / 1000).toFixed(1)} s`,
  );
  if (reportPath) console.log(`Rapport JSON: ${reportPath}`);
  if (fail === 0) {
    console.log('SUCCÈS: le JavaScript de l’interface a été exercé dans un vrai navigateur.');
  } else {
    console.log('ÉCHEC: au moins un scénario navigateur a échoué (voir les diagnostics ci-dessus).');
  }
}

async function main() {
  const options = parseCli(process.argv.slice(2));
  if (options.help) {
    printHelp();
    return EXIT.pass;
  }
  await ensureRuntime();

  const startedAt = new Date();
  const repoRoot = resolveRepoRoot();
  let discovery;
  try {
    discovery = await discoverBrowser();
  } catch (error) {
    console.error(`ÉCHEC: ${error.message}`);
    return EXIT.fail;
  }

  if (!discovery.executablePath) {
    const tried = discovery.tried.map((candidate) => `  - ${candidate.executablePath} (${candidate.source})`).join('\n');
    const message =
      'Aucun navigateur Chromium/Edge trouvé.\n' +
      'Ordre de découverte: RAVUM_BROWSER, ms-playwright chromium_headless_shell-*,\n' +
      'Chromium Playwright complet, Chrome système, Edge système, PATH (Linux).\n' +
      'Chemins essayés:\n' +
      (tried || '  (aucun)');
    if (options.requireBrowser) {
      console.error(`ÉCHEC: ${message}`);
      console.error('RAVUM_E2E_REQUIRE_BROWSER=1 / --require-browser: aucun skip silencieux autorisé.');
      return EXIT.fail;
    }
    console.error(message);
    console.error(
      'Statut: SKIP (code 2) — le JavaScript de l’interface n’a PAS été validé dans un navigateur.\n' +
        'Installez Chromium/Chrome/Edge, définissez RAVUM_BROWSER, ou relancez avec --require-browser ' +
        'pour obtenir un échec explicite.',
    );
    return EXIT.skip;
  }

  const tempRoot = await mkdtemp(path.join(tmpdir(), 'ravum-mvp0b-browser-e2e-'));
  const screenshotsDir = options.screenshots ? path.resolve(repoRoot, options.screenshots) : null;
  const reportPath = options.report ? path.resolve(repoRoot, options.report) : null;
  if (screenshotsDir) await mkdir(screenshotsDir, { recursive: true });

  const token = `e2e-token-${Math.random().toString(36).slice(2, 12)}`;
  let browser = null;
  let ctx = null;
  let results = [];
  let harnessError = null;

  console.log(`Dépôt: ${repoRoot}`);
  console.log(`Node: ${process.version} (${process.platform}, ${process.arch})`);
  console.log(`Navigateur retenu: ${discovery.executablePath}`);
  console.log(`Source: ${discovery.source}`);
  console.log(
    `Serveur: ${process.env.RAVUM_TODO_BIN ? `binaire précompilé ${process.env.RAVUM_TODO_BIN}` : 'cargo run -q -p ravum-todo --'}`,
  );

  try {
    browser = await launchBrowser(discovery);
    console.log(`Navigateur lancé: ${browser.version.product} (protocol ${browser.version.protocolVersion})`);
    ctx = new TestContext({ repoRoot, browser, tempRoot, token, screenshotsDir });
    results = await runScenarios(ctx, options);
  } catch (error) {
    harnessError = error;
    console.error(`\nÉCHEC du harnais: ${error?.stack ?? error}`);
  } finally {
    if (ctx) {
      try {
        await ctx.dispose();
      } catch (error) {
        console.error(`nettoyage du contexte: ${error.message}`);
      }
    }
    if (browser) {
      try {
        await browser.close();
      } catch (error) {
        console.error(`nettoyage navigateur: ${error.message}`);
      }
    }
    try {
      await rmWithRetry(tempRoot);
    } catch (error) {
      console.error(`nettoyage répertoire temporaire: ${error.message}`);
    }
  }

  const finishedAt = new Date();
  const durationMs = finishedAt.getTime() - startedAt.getTime();
  const report = buildReport({
    repoRoot,
    discovery,
    browser,
    results,
    startedAt,
    finishedAt,
    harnessError,
    options,
  });

  if (reportPath) {
    try {
      await mkdir(path.dirname(reportPath), { recursive: true });
      await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
    } catch (error) {
      console.error(`impossible d'écrire le rapport ${reportPath}: ${error.message}`);
      harnessError = harnessError ?? error;
    }
  }

  printSummary(results, { browser, discovery, durationMs, reportPath: reportPath ?? null });
  if (harnessError) return EXIT.fail;
  const failures = results.filter((result) => result.status === 'FAIL');
  return failures.length ? EXIT.fail : EXIT.pass;
}

const exitCode = await main().catch((error) => {
  console.error(`ÉCHEC du harnais: ${error?.stack ?? error}`);
  return EXIT.fail;
});
process.exitCode = exitCode;
