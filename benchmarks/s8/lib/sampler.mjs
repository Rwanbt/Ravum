//! Échantillonneur RAM/CPU des processus de la campagne S8.
//!
//! Un unique processus PowerShell persistant suit les PID déclarés par le
//! harnais (Node, git, ravum-collab, cargo) et découvre les descendants de
//! `cargo`/`coordinator` (rustc, binaires de test). Il écrit un JSONL de
//! WorkingSet agrégé + temps CPU cumulé ; Node calcule le pic après coup.
//!
//! L'échantillonneur n'est PAS chronométré dans les métriques de campagne : il
//! est identique pour A/B/C et documenté comme surcoût du harnais.

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { writeJsonAtomic, ensureDir, sleep } from './util.mjs';

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
      [
        '-NoProfile',
        '-NonInteractive',
        '-ExecutionPolicy',
        'Bypass',
        '-File',
        this.scriptPath,
        '-TrackFile',
        this.trackFile,
        '-OutFile',
        this.outFile,
        '-StopFile',
        this.stopFile,
        '-IntervalMs',
        String(this.intervalMs),
      ],
      { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] },
    );
    this.child.stderr.on('data', (chunk) => {
      this.stderr += chunk.toString('utf8');
    });
    this.child.on('error', (error) => {
      this.stderr += String(error);
    });
  }

  _flush() {
    if (this.writing) {
      this.pending = true;
      return;
    }
    this.writing = true;
    try {
      const entries = [...this.last.entries()].map(([pid, entry]) => ({ pid, label: entry.label, not_before_ms: entry.not_before_ms }));
      writeJsonAtomic(this.trackFile, entries);
    } catch {
      /* réessai au prochain flush */
    } finally {
      this.writing = false;
      if (this.pending) {
        this.pending = false;
        this._flush();
      }
    }
  }

  track(pid, label) {
    if (!pid) return;
    // L'horodatage de spawn permet à l'échantillonneur PowerShell de refuser un
    // PID réutilisé par un processus sans rapport (artefact observé : CPU
    // cumulé de plusieurs heures et WorkingSet de plusieurs Go sur 8 s).
    this.last.set(pid, { label, not_before_ms: Date.now() });
    this._flush();
  }

  untrack(pid) {
    if (this.last.delete(pid)) this._flush();
  }

  /** Arrête l'échantillonneur, attend son dernier tick et agrège les mesures. */
  async stop({ timeoutMs = 8000 } = {}) {
    if (!this.child) return emptyResult();
    fs.writeFileSync(this.stopFile, 'stop');
    const exited = await Promise.race([
      new Promise((resolve) => this.child.once('exit', () => resolve(true))),
      sleep(timeoutMs).then(() => false),
    ]);
    if (!exited) {
      try {
        this.child.kill('SIGKILL');
      } catch {
        /* déjà sorti */
      }
      await Promise.race([
        new Promise((resolve) => this.child.once('exit', resolve)),
        sleep(2000),
      ]);
    }
    await sleep(120); // laisse le temps au dernier flush disque
    const text = fs.existsSync(this.outFile) ? fs.readFileSync(this.outFile, 'utf8') : '';
    const samples = text
      .split(/\r?\n/)
      .filter((line) => line.trim().length > 0)
      .map((line) => {
        try {
          return JSON.parse(line);
        } catch {
          return null;
        }
      })
      .filter(Boolean);
    let peakWs = 0;
    let peakProcs = 0;
    let lastCpu = 0;
    const families = {
      coordinator: { label: 'coordinateur ravum-collab', peak_ws_bytes: 0 },
      testers: { label: 'cargo/rustc/binaires de test', peak_ws_bytes: 0 },
      git: { label: 'git', peak_ws_bytes: 0 },
      other: { label: 'autres processus suivis', peak_ws_bytes: 0 },
    };
    // Classification par nom de processus : indépendante de la propagation des
    // labels PS et robuste aux petits-enfants (rustc, binaires de test).
    const familyOf = (proc) => {
      const name = String(proc.name ?? '').toLowerCase();
      const label = String(proc.label ?? '');
      if (name.includes('ravum-collab') || /^coordinator/i.test(label)) return 'coordinator';
      if (
        name === 'cargo' ||
        name === 'rustc' ||
        name.startsWith('behavior') ||
        name.startsWith('s8_fixture') ||
        name.startsWith('s8-fixture') ||
        name === 'link' ||
        name === 'cc' ||
        /^cargo/i.test(label)
      ) {
        return 'testers';
      }
      if (name === 'git' || /^git/i.test(label)) return 'git';
      return 'other';
    };
    for (const sample of samples) {
      if (Number.isFinite(sample.ws_sum)) peakWs = Math.max(peakWs, sample.ws_sum);
      if (Number.isFinite(sample.proc_count)) peakProcs = Math.max(peakProcs, sample.proc_count);
      if (Number.isFinite(sample.cpu_total_s)) lastCpu = sample.cpu_total_s;
      if (Array.isArray(sample.procs)) {
        const totals = { coordinator: 0, testers: 0, git: 0, other: 0 };
        for (const proc of sample.procs) {
          const family = familyOf(proc);
          totals[family] += Number(proc.ws_mb) || 0; // Mo
        }
        for (const [family, value] of Object.entries(totals)) {
          families[family].peak_ws_bytes = Math.max(families[family].peak_ws_bytes, value);
        }
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
    const result = {
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
    return result;
  }
}

function emptyResult() {
  return {
    sample_count: 0,
    interval_ms: null,
    peak_ws_bytes: 0,
    peak_ws_mb: 0,
    peak_proc_count: 0,
    cpu_seconds: 0,
    duration_ms: 0,
    families: {},
    samples: [],
    stderr_tail: '',
  };
}
