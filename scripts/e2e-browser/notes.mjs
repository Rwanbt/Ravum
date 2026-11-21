// V2-L6 (T9): browser end-to-end test of `packages/http-notes-web`.
//
// Drives a real Chromium (Playwright) against the `.rv` server: add, list and
// delete notes through the page, check an error is shown for bad input, check
// the SQLite file holds exactly what the page shows, then stop the server and
// check the page can no longer reach it (no green without a server).
//
// Usage: node scripts/e2e-browser/notes.mjs [path/to/ravum-lang]
// Needs: `playwright` resolvable by Node (preinstalled in the cloud container;
// elsewhere install it outside the repository), `python3` (sqlite3 module) to
// read the database back. Adds no dependency to the repository.
// Exit code: 0 = every check passed, 1 = a check failed, 2 = setup failed.

import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const ravum = resolve(process.argv[2] ?? join(root, 'target', 'debug', 'ravum-lang'));
const work = mkdtempSync(join(tmpdir(), 'rv-e2e-'));
const db = join(work, 'notes.db');

let failures = 0;
function check(name, ok, detail = '') {
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${ok || !detail ? '' : `  (${detail})`}`);
  if (!ok) failures += 1;
}

function rowsInDb() {
  const out = execFileSync('python3', ['-I', '-c',
    'import json,sqlite3,sys; c=sqlite3.connect(sys.argv[1]);'
    + ' print(json.dumps(c.execute("SELECT id, body FROM notes ORDER BY id").fetchall()))', db]);
  return JSON.parse(out.toString());
}

// `ravum-lang run` starts the built binary as a child and does not forward a
// kill to it, so the test builds first and runs the binary itself: the
// negative control must stop the real server.
function buildServer() {
  const pkg = join(root, 'packages', 'http-notes-web');
  execFileSync(ravum, ['build', pkg], { stdio: ['ignore', 'ignore', 'inherit'] });
  return join(pkg, '.ravum', 'bin', process.platform === 'win32' ? 'http_notes_web.exe' : 'http_notes_web');
}

function startServer(binary) {
  return new Promise((resolvePort, reject) => {
    const child = spawn(binary, ['0', db], { stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    let err = '';
    const timer = setTimeout(() => reject(new Error(`server not ready in 300 s\n${out}\n${err}`)), 300_000);
    child.stdout.on('data', (chunk) => {
      out += chunk;
      const m = out.match(/^listening (\d+)$/m);
      if (m) { clearTimeout(timer); resolvePort({ child, port: Number(m[1]) }); }
    });
    child.stderr.on('data', (chunk) => { err += chunk; });
    child.on('exit', (code) => { clearTimeout(timer); reject(new Error(`server exited (${code})\n${out}\n${err}`)); });
  });
}

async function listed(page) {
  return page.$$eval('#notes li', (items) => items.map((li) => [li.dataset.id, li.querySelector('span').textContent]));
}

async function main() {
  let chromium;
  try {
    ({ chromium } = require('playwright'));
  } catch (e) {
    console.error(`setup: playwright not found (${e.message})`);
    return 2;
  }
  let server;
  try {
    server = await startServer(buildServer());
  } catch (e) {
    console.error(`setup: ${e.message}`);
    return 2;
  }
  const base = `http://127.0.0.1:${server.port}`;
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.goto(base + '/');
    await page.waitForSelector('body[data-ready="yes"]');
    check('page served by the .rv server', (await page.title()) === 'Ravum notes');
    check('empty list at start', (await listed(page)).length === 0);

    async function add(text) {
      await page.evaluate(() => { document.body.dataset.ready = 'no'; });
      await page.fill('#text', text);
      await page.click('#save');
      await page.waitForSelector('body[data-ready="yes"]');
    }

    await add('buy milk');
    await add('<b>not bold</b> & "quotes"');
    let shown = await listed(page);
    check('two notes listed after two adds', shown.length === 2, JSON.stringify(shown));
    check('text kept verbatim (no HTML injection)',
      shown[1]?.[1] === '<b>not bold</b> & "quotes"' && (await page.$$('#notes b')).length === 0);

    await add('');
    check('empty note refused with a visible error', (await page.textContent('#error')).trim() === 'empty note');
    check('refused note not listed', (await listed(page)).length === 2);

    await page.evaluate(() => { document.body.dataset.ready = 'no'; });
    await page.click('#notes li:first-child button.delete');
    await page.waitForSelector('body[data-ready="yes"]');
    shown = await listed(page);
    check('first note deleted', shown.length === 1 && shown[0][1] === '<b>not bold</b> & "quotes"',
      JSON.stringify(shown));

    const bad = await page.evaluate(async () => {
      const r = await fetch('/notes/delete', { method: 'POST', body: 'abc' });
      return [r.status, await r.text()];
    });
    check('bad id refused (400)', bad[0] === 400 && bad[1] === 'bad note id\n', JSON.stringify(bad));
    const missing = await page.evaluate(async () => (await fetch('/notes/delete', { method: 'POST', body: '999' })).status);
    check('unknown id refused (404)', missing === 404, String(missing));

    const rows = rowsInDb();
    check('SQLite file holds exactly the listed notes',
      JSON.stringify(rows.map(([id, body]) => [String(id), body])) === JSON.stringify(shown),
      `${JSON.stringify(rows)} vs ${JSON.stringify(shown)}`);

    // Negative control: with the server gone, the page must fail to load.
    server.child.removeAllListeners('exit');
    server.child.kill('SIGKILL');
    await new Promise((r) => server.child.once('exit', r));
    let reachable = true;
    try {
      await page.goto(base + '/', { timeout: 10_000 });
    } catch {
      reachable = false;
    }
    check('negative control: no server, page unreachable', !reachable);
  } finally {
    await browser.close();
    if (server && server.child.exitCode === null) server.child.kill('SIGKILL');
    rmSync(work, { recursive: true, force: true });
  }
  return failures === 0 ? 0 : 1;
}

main().then((code) => {
  console.log(code === 0 ? 'e2e-browser: all checks passed' : `e2e-browser: exit ${code}, ${failures} failed check(s)`);
  process.exit(code);
}, (e) => {
  console.error(e);
  process.exit(1);
});
