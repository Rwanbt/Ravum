// Process, port and SQLite helpers for the real-server end-to-end harness.
//
// The harness starts the actual `ravum-todo` binary (or `cargo run -q -p ravum-todo`)
// on a free loopback port, with a real SQLite file, and stops it with its whole
// process tree whatever happens.

import { spawn, spawnSync } from 'node:child_process';
import { createServer as createHttpServer, request as createHttpRequest } from 'node:http';
import { createServer as createNetServer } from 'node:net';
import { access, mkdir, rm } from 'node:fs/promises';
import { constants } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Resolve the repository root from this file location (`apps/ravum-todo/e2e/lib`). */
export function resolveRepoRoot(env = process.env) {
  if (env.RAVUM_ROOT) return path.resolve(env.RAVUM_ROOT);
  const here = path.dirname(fileURLToPath(import.meta.url));
  return path.resolve(here, '../../../..');
}

export async function rmWithRetry(target, { attempts = 16, delayMs = 125 } = {}) {
  let lastError = null;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      await rm(target, { recursive: true, force: true });
      return;
    } catch (error) {
      lastError = error;
      await sleep(delayMs);
    }
  }
  throw new Error(`impossible de supprimer ${target}: ${lastError?.message ?? 'raison inconnue'}`);
}

export async function isFile(candidate) {
  try {
    await access(candidate, constants.R_OK);
    return true;
  } catch {
    return false;
  }
}

/** Ask the OS for a currently free loopback port. */
export async function findFreePort() {
  return await new Promise((resolve, reject) => {
    const server = createNetServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      const port = address?.port;
      server.close(() => (typeof port === 'number' ? resolve(port) : reject(new Error('port introuvable'))));
    });
  });
}

function tail(text, maxChars = 4000) {
  if (!text) return '(vide)';
  const trimmed = text.trim();
  return trimmed.length <= maxChars ? trimmed : `…${trimmed.slice(-maxChars)}`;
}

/**
 * The instance command is `cargo run -q -p ravum-todo --` by default (as mandated in
 * the mission), with `RAVUM_TODO_BIN` as an explicit escape hatch: the demo script and
 * the CI build the binary once, which avoids 10+ cargo invocations and the cargo lock
 * contention on a busy shared machine.
 */
export async function resolveTodoServerCommand(root) {
  const override = process.env.RAVUM_TODO_BIN;
  if (override) {
    const binary = path.resolve(override);
    if (!(await isFile(binary))) {
      throw new Error(`RAVUM_TODO_BIN="${override}" ne pointe pas vers un fichier lisible`);
    }
    return {
      command: binary,
      prefixArgs: [],
      mode: `binaire précompilé (${path.relative(root, binary) || binary})`,
      startupTimeoutMs: 30_000,
    };
  }
  return {
    command: process.platform === 'win32' ? 'cargo.exe' : 'cargo',
    prefixArgs: ['run', '-q', '-p', 'ravum-todo', '--'],
    mode: 'cargo run -q -p ravum-todo --',
    startupTimeoutMs: Number(process.env.RAVUM_E2E_SERVER_TIMEOUT_MS ?? 180_000),
  };
}

async function waitForHealth({ baseUrl, child, logs, timeoutMs, label }) {
  const deadline = Date.now() + timeoutMs;
  let lastError = null;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) {
      throw new Error(
        `le serveur ${label} s'est arrêté prématurément (code ${child.exitCode}).\n` +
          `stderr:\n${tail(logs.stderr)}`,
      );
    }
    try {
      const response = await fetch(`${baseUrl}/health`, {
        // The reference server answers one request per connection and closes it
        // (`Connection: close`). Explicitly asking for a close avoids a Node
        // (undici) keep-alive race where a POST can observe ECONNRESET after the
        // server has already answered; browsers are not affected (see scenario 2+).
        headers: { Connection: 'close' },
        signal: AbortSignal.timeout(2000),
      });
      if (response.ok) {
        const body = await response.json();
        if (body?.status === 'ok') return body;
        lastError = new Error(`/health inattendu: ${JSON.stringify(body)}`);
      } else {
        lastError = new Error(`/health a répondu HTTP ${response.status}`);
      }
    } catch (error) {
      lastError = error;
    }
    await sleep(100);
  }
  throw new Error(
    `le serveur ${label} n'a pas répondu sur ${baseUrl}/health en ${timeoutMs} ms ` +
      `(dernière erreur: ${lastError?.message ?? 'aucune'}).\nstderr:\n${tail(logs.stderr)}`,
  );
}

/** Start the reference application on a real socket and a real SQLite file. */
export async function startTodoServer({
  root,
  dbPath,
  token,
  port,
  label = 'ravum-todo',
}) {
  const { command, prefixArgs, mode, startupTimeoutMs } = await resolveTodoServerCommand(root);
  const actualPort = port ?? (await findFreePort());
  await mkdir(path.dirname(dbPath), { recursive: true });

  const args = [
    ...prefixArgs,
    '--db',
    dbPath,
    '--port',
    String(actualPort),
    '--token',
    token,
  ];
  const child = spawn(command, args, {
    cwd: root,
    stdio: ['ignore', 'pipe', 'pipe'],
    detached: process.platform !== 'win32',
    windowsHide: true,
  });
  const logs = { stdout: '', stderr: '', command: [command, ...args].join(' ') };
  child.stdout?.setEncoding('utf8');
  child.stderr?.setEncoding('utf8');
  child.stdout?.on('data', (chunk) => {
    logs.stdout += chunk;
  });
  child.stderr?.on('data', (chunk) => {
    logs.stderr += chunk;
  });
  const exitPromise = new Promise((resolve) =>
    child.once('exit', (code, signal) => resolve({ code, signal })),
  );
  // Surface a missing command ("cargo" absent) as a clear error instead of a timeout.
  const spawnError = new Promise((_, reject) =>
    child.once('error', (error) =>
      reject(new Error(`impossible de lancer "${command}": ${error.message}`)),
    ),
  );

  const baseUrl = `http://127.0.0.1:${actualPort}`;
  const health = await Promise.race([
    spawnError,
    waitForHealth({ baseUrl, child, logs, timeoutMs: startupTimeoutMs, label }),
  ]);
  return {
    label,
    baseUrl,
    port: actualPort,
    dbPath,
    token,
    child,
    exitPromise,
    mode,
    logs,
    health,
  };
}

/**
 * Kill a child process *and its descendants*. `cargo run` is a supervisor: killing it
 * alone would leave the actual server listening. On Windows `taskkill /T /F` walks the
 * tree; on POSIX the child is spawned detached so its process group can be signalled.
 */
export async function killProcessTree(child, exitPromise) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  if (process.platform === 'win32') {
    spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], {
      stdio: 'ignore',
      windowsHide: true,
    });
  } else {
    try {
      process.kill(-child.pid, 'SIGTERM');
    } catch {
      try {
        child.kill('SIGTERM');
      } catch {
        // Already gone.
      }
    }
  }
  const exited = exitPromise
    ? await Promise.race([exitPromise.then(() => true), sleep(5000).then(() => false)])
    : false;
  if (exited) return;
  if (process.platform === 'win32') {
    spawnSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], {
      stdio: 'ignore',
      windowsHide: true,
    });
  } else {
    try {
      process.kill(-child.pid, 'SIGKILL');
    } catch {
      try {
        child.kill('SIGKILL');
      } catch {
        // Already gone.
      }
    }
  }
  if (exitPromise) await Promise.race([exitPromise, sleep(3000)]);
}

export async function stopTodoServer(server) {
  if (!server) return;
  await killProcessTree(server.child, server.exitPromise);
}

/** GET/POST JSON helper against the real server (ground truth next to the DOM checks). */
export async function fetchJson(url, { method = 'GET', token, body } = {}) {
  const headers = { Connection: 'close' };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (token) headers.Authorization = `Bearer ${token}`;
  const response = await fetch(url, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(10_000),
  });
  let payload = null;
  try {
    payload = await response.json();
  } catch {
    payload = null;
  }
  return { status: response.status, payload };
}

/** Write a database as the previous (v1) build did: no `due_date` column. */
export async function createLegacyV1Database(dbPath) {
  const { DatabaseSync } = await import('node:sqlite');
  await mkdir(path.dirname(dbPath), { recursive: true });
  const db = new DatabaseSync(dbPath);
  try {
    db.exec(
      `CREATE TABLE tasks (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           title TEXT NOT NULL,
           done INTEGER NOT NULL DEFAULT 0,
           created_at TEXT NOT NULL
       );
       INSERT INTO tasks (title, done, created_at)
           VALUES ('tâche v1 héritée', 0, '2026-11-01T10:00:00Z');
       PRAGMA user_version = 1;`,
    );
  } finally {
    db.close();
  }
}

/**
 * Tiny local HTTP proxy used by scenario 7: every request is forwarded to the real
 * server except `/api/schema`, whose `contract_version` is rewritten. This is how a
 * contract incompatibility is produced without adding a test-only flag to the server.
 */
export async function startContractProxy({ upstreamBaseUrl, contractVersion }) {
  const upstream = new URL(upstreamBaseUrl);
  const server = createHttpServer((request, response) => {
    const upstreamRequest = forwardRequest(request, response, upstream, contractVersion);
    request.pipe(upstreamRequest);
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const port = server.address()?.port;
  return {
    baseUrl: `http://127.0.0.1:${port}`,
    port,
    close: () =>
      new Promise((resolve) => {
        server.close(() => resolve());
      }),
  };
}

function forwardRequest(request, response, upstream, contractVersion) {
  const requestPath = request.url ?? '/';
  const forwarded = createHttpRequest({
    hostname: upstream.hostname,
    port: upstream.port,
    path: requestPath,
    method: request.method,
    headers: {
      ...request.headers,
      host: `${upstream.hostname}:${upstream.port}`,
      // The reference server is one-request-per-connection; ask for an explicit close
      // instead of forwarding the browser's keep-alive intent.
      connection: 'close',
    },
  });
  forwarded.on('error', (error) => {
    if (!response.headersSent) response.writeHead(502, { 'Content-Type': 'text/plain' });
    response.end(`proxy: ${error.message}`);
  });
  forwarded.on('response', (upstreamResponse) => {
    const schema = requestPath.split('?')[0] === '/api/schema';
    if (!schema) {
      response.writeHead(upstreamResponse.statusCode ?? 502, upstreamResponse.headers);
      upstreamResponse.pipe(response);
      return;
    }
    const chunks = [];
    upstreamResponse.on('data', (chunk) => chunks.push(chunk));
    upstreamResponse.on('end', () => {
      try {
        let payload;
        try {
          payload = JSON.parse(Buffer.concat(chunks).toString('utf8'));
        } catch {
          response.writeHead(upstreamResponse.statusCode ?? 502, upstreamResponse.headers);
          response.end(Buffer.concat(chunks));
          return;
        }
        payload.contract_version = contractVersion;
        const body = Buffer.from(JSON.stringify(payload));
        const headers = { ...upstreamResponse.headers, 'content-length': String(body.length) };
        delete headers['transfer-encoding'];
        delete headers['connection'];
        response.writeHead(upstreamResponse.statusCode ?? 200, headers);
        response.end(body);
      } catch (error) {
        if (!response.headersSent) {
          response.writeHead(502, { 'Content-Type': 'text/plain; charset=utf-8' });
        }
        response.end(`proxy: ${error.message}`);
      }
    });
  });
  return forwarded;
}
