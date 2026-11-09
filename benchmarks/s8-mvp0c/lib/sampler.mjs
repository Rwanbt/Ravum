//! Échantillonneur RAM/CPU des processus d'une campagne S8 MVP-0c.
//!
//! Un unique processus PowerShell persistant suit les PID déclarés (agents,
//! cargo). Les descendants (rustc, binaires de test) ne sont PAS isolables sans WMI :
//! limite déclarée dans la preuve ; Node mesure aussi la charge machine complète.
//!
//! L'échantillonneur est appelé UNIQUEMENT par le biais du harnais : il tue son
//! processus à la fin (`stop`), et un timeout de sécurité évite tout processus
//! résiduel.

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { ensureDir, sleep, writeJsonAtomic } from './fsutil.mjs';

export class ProcessSampler {
  constructor({ dir, psPath, scriptPath, intervalMs = 250 }) {
    this.dir = dir;
    this.psPath = psPath;
    this.scriptPath = scriptPath;
    this.intervalMs = intervalMs;
    this.trackFile = path.join(dir, 'tracked.json');
    this.outFile = path.join(dir, 'samples.jsonl');
    this.stopFile = path.join(dir, 'stop.flag');
    this.last = new Map();
    this.child = null;
    this.stderr = '';
    this.writing = false;
    this.pending = false;
  }

  start() {
    ensureDir(this.dir);
    writeJsonAtomic(this.trackFile, []);
    this.child = spawn(
      this.psPath,
      ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', this.scriptPath, '-TrackFile', this.trackFile, '-OutFile', this.outFile, '-StopFile', this.stopFile, '-IntervalMs', String(this.intervalMs)],
      { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] },
    );
    this.child.stderr.on('data', (chunk) => { this.stderr += chunk.toString('utf8'); });
    this.child.on('error', (error) => { this.stderr += String(error); });
  }

  _flush() {
    if (this.writing) { this.pending = true; return; }
    this.writing = true;
    try {
      const entries = [...this.last.entries()].map(([pid, entry]) => ({ pid, label: entry.label, not_before_ms: entry.not_before_ms }));
      writeJsonAtomic(this.trackFile, entries);
    } catch {
      /* réessai au prochain flush */
    } finally {
      this.writing = false;
      if (this.pending) { this.pending = false; this._flush(); }
    }
  }

  track(pid, label) {
    if (!pid) return;
    this.last.set(pid, { label, not_before_ms: Date.now() });
    this._flush();
  }

  untrack(pid) {
    if (this.last.delete(pid)) this._flush();
  }

  /** Attend le premier échantillon (démarrage PowerShell) avant de mesurer. */
  async waitForReady({ timeoutMs = 10000 } = {}) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      try {
        if (fs.existsSync(this.outFile) && fs.statSync(this.outFile).size > 0) return true;
      } catch {
        /* fichier en cours de création */
      }
      await sleep(40);
    }
    return false;
  }

  async stop({ timeoutMs = 8000 } = {}) {
    if (!this.child) return emptySamplerResult();
    fs.writeFileSync(this.stopFile, 'stop');
    const exited = await Promise.race([
      new Promise((resolve) => this.child.once('exit', () => resolve(true))),
      sleep(timeoutMs).then(() => false),
    ]);
    if (!exited) {
      try { this.child.kill('SIGKILL'); } catch { /* déjà sorti */ }
      await Promise.race([new Promise((resolve) => this.child.once('exit', resolve)), sleep(2000)]);
    }
    await sleep(120);
    const text = fs.existsSync(this.outFile) ? fs.readFileSync(this.outFile, 'utf8') : '';
    const samples = text
      .split(/\r?\n/)
      .filter((line) => line.trim().length > 0)
      .map((line) => { try { return JSON.parse(line); } catch { return null; } })
      .filter(Boolean);
    let peakWs = 0;
    let peakProcs = 0;
    let lastCpu = 0;
    const families = {
      agents: { label: 'agents Node (processus directs)', peak_ws_bytes: 0 },
      testers: { label: 'cargo (direct ; rustc non isolable sans WMI)', peak_ws_bytes: 0 },
      other: { label: 'autres', peak_ws_bytes: 0 },
    };
    const familyOf = (proc) => {
      const name = String(proc.name ?? '').toLowerCase();
      const label = String(proc.label ?? '');
      if (name === 'cargo') return 'testers';
      if (name === 'node' || /^agent/.test(label)) return 'agents';
      return 'other';
    };
    for (const sample of samples) {
      if (Number.isFinite(sample.ws_sum)) peakWs = Math.max(peakWs, sample.ws_sum);
      if (Number.isFinite(sample.proc_count)) peakProcs = Math.max(peakProcs, sample.proc_count);
      if (Number.isFinite(sample.cpu_total_s)) lastCpu = sample.cpu_total_s;
      if (Array.isArray(sample.procs)) {
        const totals = { agents: 0, testers: 0, other: 0 };
        for (const proc of sample.procs) totals[familyOf(proc)] += Number(proc.ws_mb) || 0;
        for (const [family, value] of Object.entries(totals)) families[family].peak_ws_bytes = Math.max(families[family].peak_ws_bytes, value);
      }
    }
    const familyReport = {};
    for (const [family, entry] of Object.entries(families)) {
      familyReport[family] = {
        label: entry.label,
        peak_ws_bytes: Math.round(entry.peak_ws_bytes * 1024 * 1024),
        peak_ws_mb: Math.round(entry.peak_ws_bytes * 10) / 10,
      };
    }
    return {
      sample_count: samples.length,
      interval_ms: this.intervalMs,
      peak_ws_bytes: peakWs,
      peak_ws_mb: Math.round((peakWs / (1024 * 1024)) * 10) / 10,
      peak_proc_count: peakProcs,
      cpu_seconds: lastCpu,
      duration_ms: samples.length > 0 ? samples[samples.length - 1].t_ms : 0,
      families: familyReport,
      samples,
      stderr_tail: this.stderr.slice(-2000),
    };
  }
}

export function emptySamplerResult() {
  return { sample_count: 0, interval_ms: null, peak_ws_bytes: 0, peak_ws_mb: 0, peak_proc_count: 0, cpu_seconds: 0, duration_ms: 0, families: {}, samples: [], stderr_tail: '' };
}
