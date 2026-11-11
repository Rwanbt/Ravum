//! Environnement machine et charge — MESURES NODE UNIQUEMENT.
//!
//! Contrainte observée pendant la mission : les requêtes CIM/WMI
//! (`Get-CimInstance Win32_*`) peuvent se bloquer ou échouer sur la machine
//! partagée. Le harnais n'en dépend donc plus :
//! * `machineInfo` lit `os` (CPU, RAM, OS) + versions d'outils + disque ;
//! * `probeLoad` mesure la charge avec deux instantanés de `os.cpus()` espacés
//!   de 250 ms et `os.freemem()` (aucun sous-processus) ;
//! * `lightSample` sert au suivi continu de la charge pendant les scénarios.

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import { performance } from 'node:perf_hooks';
import { run } from './exec.mjs';
import { round, sleep } from './fsutil.mjs';

/** Localise PowerShell (utilisé seulement pour l'échantillonneur Get-Process). */
export function findPowerShell() {
  for (const candidate of ['pwsh', 'powershell']) {
    const probe = spawnSync(candidate, ['-NoProfile', '-NonInteractive', '-Command', '$PSVersionTable.PSVersion.Major'], {
      windowsHide: true,
      encoding: 'utf8',
      timeout: 20000,
    });
    if (probe.status === 0) return { path: candidate, version: (probe.stdout || '').trim() };
  }
  throw new Error('aucun PowerShell utilisable (pwsh ni powershell) dans PATH');
}

/** Instantané bon marché (pas de sous-processus). */
export function lightSample() {
  const cpus = os.cpus();
  let total = 0;
  let idle = 0;
  for (const cpu of cpus) {
    for (const value of Object.values(cpu.times)) total += value;
    idle += cpu.times.idle;
  }
  return {
    epoch_ms: Date.now(),
    free_ram_bytes: os.freemem(),
    total_ram_bytes: os.totalmem(),
    cpu_total_ticks: total,
    cpu_idle_ticks: idle,
  };
}

export function cpuBusyPct(before, after) {
  const total = after.cpu_total_ticks - before.cpu_total_ticks;
  const idle = after.cpu_idle_ticks - before.cpu_idle_ticks;
  if (total <= 0) return null;
  return round((1 - idle / total) * 100, 1);
}

/** Sonde de charge : deux instantanés espacés de 250 ms (Node only). */
export async function probeLoad() {
  const started = performance.now();
  const before = lightSample();
  await sleep(250);
  const after = lightSample();
  return {
    ok: true,
    source: 'node-os',
    ms: round(performance.now() - started),
    epoch_ms: after.epoch_ms,
    free_ram_bytes: after.free_ram_bytes,
    total_ram_bytes: after.total_ram_bytes,
    load_pct: cpuBusyPct(before, after),
    logical_cpus: os.cpus().length,
  };
}

/** Informations machine (Node only + versions d'outils). */
export async function machineInfo({ log = null } = {}) {
  const cpus = os.cpus();
  const versions = {};
  for (const [name, cmd, args] of [
    ['node', process.execPath, ['--version']],
    ['cargo', 'cargo', ['--version']],
    ['rustc', 'rustc', ['--version']],
    ['git', 'git', ['--version']],
  ]) {
    const result = await run(cmd, args, { timeoutMs: 60000, log, label: `version-${name}` });
    versions[name] = result.code === 0 ? result.stdout.trim() : `<indisponible: ${result.code}>`;
  }
  let disk = null;
  try {
    const stat = fs.statfsSync(process.cwd());
    disk = { free_bytes: stat.bfree * stat.bsize, total_bytes: stat.blocks * stat.bsize };
  } catch {
    disk = null;
  }
  return {
    source: 'node-os',
    cpu_model: cpus[0]?.model ?? 'unknown',
    logical_cores: cpus.length,
    physical_cores: null,
    physical_cores_note: 'non lisible sans WMI (indisponible pendant la mission) : seule la valeur logique est mesurée',
    total_ram_bytes: os.totalmem(),
    os_platform: process.platform,
    os_release: os.release(),
    os_version: typeof os.version === 'function' ? os.version() : null,
    hostname: os.hostname(),
    versions,
    disk,
    node: { version: process.version, platform: process.platform, arch: process.arch },
  };
}

/** Suivi continu de la charge machine pendant une phase (150 ms). */
export function startMachineWatch({ intervalMs = 150 } = {}) {
  const samples = [lightSample()];
  const timer = setInterval(() => {
    samples.push(lightSample());
  }, intervalMs);
  timer.unref?.();
  return {
    stop() {
      clearInterval(timer);
      samples.push(lightSample());
      let minFree = Infinity;
      let maxCpu = 0;
      for (let i = 0; i < samples.length; i += 1) {
        minFree = Math.min(minFree, samples[i].free_ram_bytes);
        if (i > 0) {
          const busy = cpuBusyPct(samples[i - 1], samples[i]);
          if (busy !== null) maxCpu = Math.max(maxCpu, busy);
        }
      }
      const baseline = samples[0].free_ram_bytes;
      return {
        sample_count: samples.length,
        interval_ms: intervalMs,
        machine_min_free_ram_bytes: Number.isFinite(minFree) ? minFree : null,
        machine_ram_drop_bytes: Number.isFinite(minFree) ? Math.max(0, baseline - minFree) : null,
        machine_peak_cpu_busy_pct: round(maxCpu),
        source: 'node-os',
        samples,
      };
    },
  };
}
