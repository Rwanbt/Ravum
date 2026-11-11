// Browser discovery, launch and session management for the CDP end-to-end harness.
//
// Zero npm dependency: the harness controls a real Chromium/Edge/Chrome headless
// binary directly through the DevTools protocol. Discovery order:
//   1. `RAVUM_BROWSER` (explicit executable path);
//   2. Playwright headless shells (`ms-playwright/chromium_headless_shell-*`), newest
//      revision first, both the historical `chrome-win/headless_shell.exe` layout and
//      the current `chrome-headless-shell-win64/chrome-headless-shell.exe` one;
//   3. Chromium installed by Playwright (last-resort real Chromium);
//   4. system Chrome / Edge (`C:\Program Files...`);
//   5. on Linux, `chromium` / `google-chrome` / `microsoft-edge` from PATH, then the
//      Playwright cache under `~/.cache/ms-playwright`.
//
// If nothing is found the caller must report an explicit SKIP (or fail when the
// environment requires a browser); there is no silent green.

import { spawn } from 'node:child_process';
import { access, readdir, mkdtemp, rm } from 'node:fs/promises';
import { constants } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import path from 'node:path';
import { CdpConnection } from './cdp.mjs';
import { Page } from './page.mjs';
import { killProcessTree, rmWithRetry, sleep } from './server.mjs';

const LAUNCH_TIMEOUT_MS = 30_000;

async function isExecutable(candidate) {
  try {
    await access(candidate, process.platform === 'win32' ? constants.F_OK : constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

/** Directory names matching `pattern` under `root`, sorted by their numeric suffix (desc). */
async function revisionsDescending(root, prefix) {
  let entries;
  try {
    entries = await readdir(root, { withFileTypes: true });
  } catch {
    return [];
  }
  return entries
    .filter((entry) => entry.isDirectory() && entry.name.startsWith(prefix))
    .map((entry) => ({ name: entry.name, revision: Number(entry.name.slice(prefix.length)) }))
    .filter((entry) => Number.isFinite(entry.revision))
    .sort((left, right) => right.revision - left.revision);
}

async function whichFromPath(name) {
  const pathValue = process.env.PATH ?? '';
  for (const directory of pathValue.split(path.delimiter)) {
    if (!directory) continue;
    const candidate = path.join(directory, name);
    if (await isExecutable(candidate)) return candidate;
  }
  return null;
}

function browserKind(executablePath) {
  const name = path.basename(executablePath).toLowerCase();
  if (name.includes('headless-shell') || name.includes('headless_shell')) return 'headless-shell';
  if (name.includes('msedge')) return 'edge';
  if (name.includes('chrome')) return 'chrome';
  return 'chromium';
}

async function* windowsCandidates() {
  const roots = [
    process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, 'ms-playwright'),
    process.env.USERPROFILE && path.join(process.env.USERPROFILE, 'AppData', 'Local', 'ms-playwright'),
  ].filter(Boolean);

  for (const root of roots) {
    for (const revision of await revisionsDescending(root, 'chromium_headless_shell-')) {
      for (const relative of [
        ['chrome-headless-shell-win64', 'chrome-headless-shell.exe'],
        ['chrome-headless-shell-win32', 'chrome-headless-shell.exe'],
        ['chrome-win64', 'headless_shell.exe'],
        ['chrome-win', 'headless_shell.exe'],
      ]) {
        yield {
          executablePath: path.join(root, revision.name, ...relative),
          source: `ms-playwright ${revision.name} (headless shell)`,
          kind: 'headless-shell',
        };
      }
    }
  }

  // System browsers come before a full Playwright Chromium (per the mission order).
  for (const executablePath of [
    'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    'C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  ]) {
    yield { executablePath, source: 'navigateur système (Chemin standard Windows)', kind: browserKind(executablePath) };
  }

  // Last real-Chromium fallback: the full build installed by Playwright.
  for (const root of roots) {
    for (const revision of await revisionsDescending(root, 'chromium-')) {
      for (const relative of [
        ['chrome-win64', 'chrome.exe'],
        ['chrome-win', 'chrome.exe'],
      ]) {
        yield {
          executablePath: path.join(root, revision.name, ...relative),
          source: `ms-playwright ${revision.name} (Chromium complet)`,
          kind: 'chrome',
        };
      }
    }
  }
}

async function* posixCandidates() {
  for (const name of [
    'chromium',
    'chromium-browser',
    'google-chrome',
    'google-chrome-stable',
    'microsoft-edge',
    'microsoft-edge-stable',
  ]) {
    const executablePath = await whichFromPath(name);
    if (executablePath) {
      yield { executablePath, source: `PATH:${name}`, kind: browserKind(executablePath) };
    }
  }

  const roots = [
    process.env.PLAYWRIGHT_BROWSERS_PATH,
    path.join(homedir(), '.cache', 'ms-playwright'),
    path.join(homedir(), 'Library', 'Caches', 'ms-playwright'),
  ].filter(Boolean);
  for (const root of roots) {
    for (const revision of await revisionsDescending(root, 'chromium_headless_shell-')) {
      for (const relative of [
        ['chrome-headless-shell-linux64', 'chrome-headless-shell'],
        ['chrome-linux', 'headless_shell'],
      ]) {
        yield {
          executablePath: path.join(root, revision.name, ...relative),
          source: `ms-playwright ${revision.name} (headless shell)`,
          kind: 'headless-shell',
        };
      }
    }
    for (const revision of await revisionsDescending(root, 'chromium-')) {
      for (const relative of [
        ['chrome-linux64', 'chrome'],
        ['chrome-linux', 'chrome'],
      ]) {
        yield {
          executablePath: path.join(root, revision.name, ...relative),
          source: `ms-playwright ${revision.name} (Chromium complet)`,
          kind: 'chrome',
        };
      }
    }
  }
}

/**
 * Resolve a browser executable. Returns `{ executablePath, source, kind, tried }` or
 * `{ executablePath: null, tried }`; throws only when `RAVUM_BROWSER` is set but invalid
 * (that is a configuration error, not a missing browser).
 */
export async function discoverBrowser() {
  const tried = [];
  const explicit = process.env.RAVUM_BROWSER;
  if (explicit) {
    const executablePath = path.resolve(explicit.trim().replace(/^"(.*)"$/, '$1'));
    tried.push({ executablePath, source: 'RAVUM_BROWSER' });
    if (await isExecutable(executablePath)) {
      return { executablePath, source: 'RAVUM_BROWSER', kind: browserKind(executablePath), tried };
    }
    throw new Error(
      `RAVUM_BROWSER="${explicit}" ne pointe pas vers un exécutable lisible; ` +
        `corrigez la variable ou retirez-la pour laisser la découverte automatique.`,
    );
  }

  const candidates = process.platform === 'win32' ? windowsCandidates() : posixCandidates();
  for await (const candidate of candidates) {
    tried.push({ executablePath: candidate.executablePath, source: candidate.source });
    if (await isExecutable(candidate.executablePath)) {
      return {
        executablePath: candidate.executablePath,
        source: candidate.source,
        kind: candidate.kind,
        tried,
      };
    }
  }
  return { executablePath: null, tried };
}

export class BrowserSession {
  constructor({ child, exitPromise, profileDir, connection, discovery, version, arguments: launchArguments }) {
    this.child = child;
    this.exitPromise = exitPromise;
    this.profileDir = profileDir;
    this.connection = connection;
    this.discovery = discovery;
    this.version = version;
    this.launchArguments = launchArguments;
    this.pages = [];
  }

  /** Open one fresh tab and attach a flattened CDP session to it. */
  async newPage({ url, label = `page-${this.pages.length + 1}` } = {}) {
    const { targetId } = await this.connection.send('Target.createTarget', { url: 'about:blank' });
    const { sessionId } = await this.connection.send('Target.attachToTarget', {
      targetId,
      flatten: true,
    });
    const page = await Page.create({ connection: this.connection, targetId, sessionId, label, url });
    this.pages.push(page);
    return page;
  }

  async close() {
    try {
      await Promise.race([this.connection.send('Browser.close', {}, { timeoutMs: 2000 }), sleep(1500)]);
    } catch {
      // Graceful close is best effort: the process is killed below anyway.
    }
    try {
      this.connection.close();
    } catch {
      // Already closed.
    }
    await killProcessTree(this.child, this.exitPromise);
    await rmWithRetry(this.profileDir);
  }
}

/**
 * Launch the discovered browser headless with the mandated flags and connect to the
 * browser-level DevTools endpoint. The DevTools URL is read from stderr, which is how
 * `--remote-debugging-port=0` advertises the OS-chosen port.
 */
export async function launchBrowser(discovery) {
  const profileDir = await mkdtemp(path.join(tmpdir(), 'ravum-browser-e2e-profile-'));
  const launchArguments = [
    '--headless=new',
    '--remote-debugging-port=0',
    '--no-first-run',
    '--no-default-browser-check',
    `--user-data-dir=${profileDir}`,
    '--disable-gpu',
    '--disable-dev-shm-usage',
    'about:blank',
  ];
  if (process.env.RAVUM_BROWSER_NO_SANDBOX === '1') launchArguments.unshift('--no-sandbox');
  if (process.env.RAVUM_BROWSER_EXTRA_ARGS) {
    launchArguments.unshift(...process.env.RAVUM_BROWSER_EXTRA_ARGS.split(/\s+/).filter(Boolean));
  }

  const child = spawn(discovery.executablePath, launchArguments, {
    stdio: ['ignore', 'pipe', 'pipe'],
    detached: process.platform !== 'win32',
    windowsHide: true,
  });
  const stderr = [];
  let stderrText = '';
  child.stdout?.resume();
  child.stderr?.setEncoding('utf8');
  child.stderr?.on('data', (chunk) => {
    stderrText += chunk;
    stderr.push(chunk);
  });
  const exitPromise = new Promise((resolve) =>
    child.once('exit', (code, signal) => resolve({ code, signal })),
  );
  const spawnError = new Promise((_, reject) =>
    child.once('error', (error) =>
      reject(new Error(`impossible de lancer le navigateur "${discovery.executablePath}": ${error.message}`)),
    ),
  );

  const devtoolsUrl = await Promise.race([
    spawnError,
    (async () => {
      const deadline = Date.now() + LAUNCH_TIMEOUT_MS;
      while (Date.now() < deadline) {
        const match = /DevTools listening on (ws:\/\/\S+)/.exec(stderrText);
        if (match) return match[1];
        if (child.exitCode !== null) {
          throw new Error(
            `le navigateur s'est arrêté au lancement (code ${child.exitCode}).\n` +
              `stderr:\n${stderrText.trim() || '(vide)'}`,
          );
        }
        await sleep(50);
      }
      throw new Error(
        `le navigateur n'a pas annoncé d'URL DevTools en ${LAUNCH_TIMEOUT_MS} ms.\n` +
          `stderr:\n${stderrText.trim() || '(vide)'}`,
      );
    })(),
  ]);

  let connection;
  try {
    connection = await CdpConnection.connect(devtoolsUrl, { timeoutMs: 15_000 });
    const version = await connection.send('Browser.getVersion');
    return new BrowserSession({
      child,
      exitPromise,
      profileDir,
      connection,
      discovery,
      version,
      arguments: launchArguments,
    });
  } catch (error) {
    connection?.close();
    await killProcessTree(child, exitPromise);
    await rmWithRetry(profileDir);
    throw error;
  }
}
