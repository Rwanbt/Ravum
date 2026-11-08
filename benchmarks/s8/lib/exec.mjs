//! Exécution de commandes instrumentée pour le harnais S8.
//!
//! Chaque commande est mesurée (wall-clock), journalisée pour les preuves et,
//! si un tracker est fourni, son PID est suivi par l'échantillonneur RAM/CPU.

import { spawn } from 'node:child_process';
import { performance } from 'node:perf_hooks';

export class Tracker {
  constructor(sampler = null, sanitize = (x) => x) {
    this.sampler = sampler;
    this.sanitize = sanitize;
    this.live = new Map();
  }
  add(pid, label) {
    if (!pid) return;
    this.live.set(pid, label);
    if (this.sampler) this.sampler.track(pid, label);
  }
  remove(pid) {
    if (!pid || !this.live.has(pid)) return;
    this.live.delete(pid);
    if (this.sampler) this.sampler.untrack(pid);
  }
}

export class CommandLog {
  constructor(maxEntries = 600) {
    this.entries = [];
    this.maxEntries = maxEntries;
    this.dropped = 0;
  }
  add(entry) {
    if (this.entries.length < this.maxEntries) this.entries.push(entry);
    else this.dropped += 1;
  }
}

/**
 * Lance une commande et attend sa fin.
 * @returns {Promise<{cmd:string,args:string[],code:number|null,signal:string|null,
 *   ms:number,timedOut:boolean,stdout:string,stderr:string,truncated:boolean,error?:string}>}
 */
export function run(cmd, args, options = {}) {
  const {
    cwd,
    env,
    timeoutMs = 600000,
    log = null,
    tracker = null,
    label = null,
    input = null,
    maxOut = 512 * 1024,
    sanitize = (x) => x,
  } = options;

  const startedEpoch = Date.now();
  const t0 = performance.now();

  return new Promise((resolve) => {
    let child;
    try {
      child = spawn(cmd, args, {
        cwd,
        env: env ?? process.env,
        windowsHide: true,
        stdio: ['pipe', 'pipe', 'pipe'],
      });
    } catch (error) {
      resolve({
        cmd,
        args,
        code: null,
        signal: null,
        ms: performance.now() - t0,
        timedOut: false,
        stdout: '',
        stderr: '',
        truncated: false,
        error: String(error),
        startedEpoch,
      });
      return;
    }

    const trackedLabel = label ?? cmd;
    if (tracker) tracker.add(child.pid, trackedLabel);

    let stdout = '';
    let stderr = '';
    let truncated = false;
    let settled = false;

    child.stdout.on('data', (chunk) => {
      if (stdout.length < maxOut) stdout += chunk.toString('utf8');
      else truncated = true;
    });
    child.stderr.on('data', (chunk) => {
      if (stderr.length < maxOut) stderr += chunk.toString('utf8');
      else truncated = true;
    });

    let timer = null;
    let timedOut = false;
    if (timeoutMs > 0) {
      timer = setTimeout(() => {
        timedOut = true;
        try {
          child.kill('SIGKILL');
        } catch {
          /* déjà terminé */
        }
      }, timeoutMs);
      timer.unref?.();
    }

    const finish = (code, signal, error) => {
      if (settled) return;
      settled = true;
      if (timer) clearTimeout(timer);
      if (tracker) tracker.remove(child.pid);
      const ms = performance.now() - t0;
      const entry = {
        label: trackedLabel,
        command: sanitize([cmd, ...args].join(' ')),
        cwd: sanitize(cwd ?? process.cwd()),
        ms: Math.round(ms * 1000) / 1000,
        code,
        signal,
        timedOut,
        started_epoch_ms: startedEpoch,
        ...(error ? { error } : {}),
      };
      if (log) log.add(entry);
      resolve({
        cmd,
        args,
        code,
        signal,
        ms,
        timedOut,
        stdout: sanitize(stdout),
        stderr: sanitize(stderr),
        truncated,
        startedEpoch,
        ...(error ? { error } : {}),
      });
    };

    child.on('error', (error) => finish(null, null, String(error)));
    child.on('close', (code, signal) => finish(code, signal));
    if (input !== null && input !== undefined) child.stdin.write(input);
    child.stdin.end();
  });
}

/** Lance une commande qui DOIT réussir, sinon lève une erreur explicite. */
export async function runOk(cmd, args, options = {}) {
  const result = await run(cmd, args, options);
  if (result.code !== 0) {
    throw new Error(
      `commande échouée (code ${result.code}${result.timedOut ? ', timeout' : ''}) : ` +
        `${[cmd, ...args].join(' ')}\n--- stdout ---\n${result.stdout}\n--- stderr ---\n${result.stderr}`,
    );
  }
  return result;
}

/** Exécute un segment et renvoie sa durée wall-clock en ms. */
export async function timed(fn) {
  const t0 = performance.now();
  const value = await fn();
  return { value, ms: performance.now() - t0 };
}
