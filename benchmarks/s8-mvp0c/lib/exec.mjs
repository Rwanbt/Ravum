//! Exécution de commandes instrumentée (wall-clock, journal, tracking PID).

import { spawn } from 'node:child_process';
import { performance } from 'node:perf_hooks';

export class CommandLog {
  constructor(maxEntries = 500) {
    this.entries = [];
    this.maxEntries = maxEntries;
    this.dropped = 0;
  }
  add(entry) {
    if (this.entries.length < this.maxEntries) this.entries.push(entry);
    else this.dropped += 1;
  }
}

export class Tracker {
  constructor(sampler = null) {
    this.sampler = sampler;
    this.live = new Set();
  }
  add(pid, label) {
    if (!pid) return;
    this.live.add(pid);
    if (this.sampler) this.sampler.track(pid, label);
  }
  remove(pid) {
    if (!pid || !this.live.has(pid)) return;
    this.live.delete(pid);
    if (this.sampler) this.sampler.untrack(pid);
  }
}

/**
 * Lance une commande et attend sa fin.
 * @returns {Promise<{cmd:string,args:string[],code:number|null,ms:number,timedOut:boolean,
 *   stdout:string,stderr:string,error?:string}>}
 */
export function run(cmd, args, options = {}) {
  const { cwd, env, timeoutMs = 900000, label = null, log = null, tracker = null, input = null, maxOut = 512 * 1024, sanitize = (x) => x } = options;
  const startedAt = Date.now();
  const t0 = performance.now();
  return new Promise((resolve) => {
    let child;
    try {
      child = spawn(cmd, args, { cwd, env: env ?? process.env, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
    } catch (error) {
      resolve({ cmd, args, code: null, ms: performance.now() - t0, timedOut: false, stdout: '', stderr: '', error: String(error), startedAt });
      return;
    }
    if (tracker) tracker.add(child.pid, label ?? cmd);
    let stdout = '';
    let stderr = '';
    let settled = false;
    let timedOut = false;
    child.stdout.on('data', (chunk) => { if (stdout.length < maxOut) stdout += chunk.toString('utf8'); });
    child.stderr.on('data', (chunk) => { if (stderr.length < maxOut) stderr += chunk.toString('utf8'); });
    let timer = null;
    if (timeoutMs > 0) {
      timer = setTimeout(() => {
        timedOut = true;
        try { child.kill('SIGKILL'); } catch { /* déjà sorti */ }
      }, timeoutMs);
      timer.unref?.();
    }
    const finish = (code, error) => {
      if (settled) return;
      settled = true;
      if (timer) clearTimeout(timer);
      if (tracker) tracker.remove(child.pid);
      const entry = {
        label: label ?? cmd,
        command: sanitize([cmd, ...args].join(' ')),
        cwd: sanitize(cwd ?? process.cwd()),
        ms: Math.round((performance.now() - t0) * 1000) / 1000,
        code,
        timedOut,
        started_at_epoch_ms: startedAt,
        ...(error ? { error } : {}),
      };
      if (log) log.add(entry);
      resolve({ cmd, args, code, ms: performance.now() - t0, timedOut, stdout: sanitize(stdout), stderr: sanitize(stderr), startedAt, ...(error ? { error } : {}) });
    };
    child.on('error', (error) => finish(null, String(error)));
    child.on('close', (code) => finish(code));
    if (input !== null && input !== undefined) child.stdin.write(input);
    child.stdin.end();
  });
}

export async function runOk(cmd, args, options = {}) {
  const result = await run(cmd, args, options);
  if (result.code !== 0) {
    throw new Error(
      `commande échouée (code ${result.code}${result.timedOut ? ', timeout' : ''}) : ${[cmd, ...args].join(' ')}\n--- stdout ---\n${result.stdout}\n--- stderr ---\n${result.stderr}`,
    );
  }
  return result;
}

/** Spawne une commande sans attendre, avec promesse de fin ({code,ms}). */
export function spawnAsync(cmd, args, options = {}) {
  return run(cmd, args, options);
}

export function nowEpochMs() {
  return Date.now();
}

/** Parse la sortie de `cargo test --quiet` (tous les binaires de test). */
export function parseCargoTest(stdout, stderr, exitCode) {
  const combined = `${stdout}\n${stderr}`;
  const lines = [...combined.matchAll(/test result: (ok|FAILED)\. (\d+) passed; (\d+) failed/g)];
  let passed = 0;
  let failed = 0;
  let allOk = true;
  for (const line of lines) {
    passed += Number(line[2]);
    failed += Number(line[3]);
    if (line[1] !== 'ok') allOk = false;
  }
  const failedNames = [];
  for (const match of combined.matchAll(/^\s{4}([A-Za-z0-9_]+)\s*$/gm)) failedNames.push(match[1]);
  return {
    green: exitCode === 0 && lines.length > 0 && allOk && failed === 0 && passed > 0,
    passed,
    failed,
    test_targets: lines.length,
    failed_names: [...new Set(failedNames)].slice(0, 40),
    exit_code: exitCode,
  };
}
