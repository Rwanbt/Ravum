//! Environnement et charge machine du benchmark S8.
//!
//! Deux niveaux de mesure, volontairement séparés :
//! * `probeSystemLoad()` — sonde PowerShell complète (RAM libre, LoadPercentage,
//!   nombre de processus, top CPU) coûteuse (~1,5 s) : utilisée au début et à la
//!   fin de la campagne globale et au point milieu.
//! * `lightHostSample()` — instantané Node sans sous-processus (`os.freemem()`,
//!   temps CPU cumulés de `os.cpus()`) : pris avant/après chaque répétition pour
//!   documenter la dérive de charge entre A1 et C5.

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import { run } from './exec.mjs';
import { round } from './util.mjs';

export function findPowerShell() {
  for (const candidate of ['pwsh', 'powershell']) {
    const probe = spawnSync(candidate, ['-NoProfile', '-NonInteractive', '-Command', '$PSVersionTable.PSVersion.Major'], {
      windowsHide: true,
      encoding: 'utf8',
      timeout: 20000,
    });
    if (probe.status === 0) {
      const version = (probe.stdout || '').trim();
      return { path: candidate, version };
    }
  }
  throw new Error('aucun PowerShell utilisable (pwsh ni powershell) trouvé dans PATH');
}

const LOAD_SCRIPT = `
$ErrorActionPreference = 'SilentlyContinue'
$os = Get-CimInstance Win32_OperatingSystem
$cs = Get-CimInstance Win32_ComputerSystem
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$load = (Get-CimInstance Win32_Processor | Measure-Object -Property LoadPercentage -Average).Average
$procs = (Get-Process).Count
$top = @(Get-Process | Sort-Object CPU -Descending | Select-Object -First 5 | ForEach-Object {
    [pscustomobject]@{ name = $_.ProcessName; pid = $_.Id; cpu_s = [Math]::Round($_.CPU, 1); ws_mb = [Math]::Round($_.WorkingSet64 / 1MB, 1) }
})
[pscustomobject]@{
    epoch_ms        = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    free_ram_bytes  = [int64]$os.FreePhysicalMemory * 1024
    total_ram_bytes = [int64]$cs.TotalPhysicalMemory
    load_pct        = if ($null -eq $load) { $null } else { [Math]::Round([double]$load, 1) }
    process_count   = $procs
    cpu_name        = $cpu.Name
    top_processes   = $top
} | ConvertTo-Json -Depth 5 -Compress
`;

/** Sonde complète (PowerShell). ~1,5 s sous charge ; appelée peu souvent. */
export async function probeSystemLoad(ps, { log = null } = {}) {
  const result = await run(ps.path, ['-NoProfile', '-NonInteractive', '-Command', LOAD_SCRIPT], {
    timeoutMs: 120000,
    log,
    label: 'load-probe',
  });
  if (result.code !== 0) {
    return { ok: false, ms: round(result.ms), error: result.stderr.slice(-500) };
  }
  try {
    const parsed = JSON.parse(result.stdout.trim());
    return { ok: true, ms: round(result.ms), ...parsed };
  } catch (error) {
    return { ok: false, ms: round(result.ms), error: `JSON invalide: ${String(error)}`, raw: result.stdout.slice(0, 500) };
  }
}

const MACHINE_SCRIPT = `
$ErrorActionPreference = 'SilentlyContinue'
$cs = Get-CimInstance Win32_ComputerSystem
$os = Get-CimInstance Win32_OperatingSystem
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
[pscustomobject]@{
    cpu_name          = $cpu.Name
    physical_cores    = $cpu.NumberOfCores
    logical_cores     = $cpu.NumberOfLogicalProcessors
    total_ram_bytes   = [int64]$cs.TotalPhysicalMemory
    os_caption        = $os.Caption
    os_version        = $os.Version
    os_build          = $os.BuildNumber
    hostname          = $env:COMPUTERNAME
    power_plan        = (powercfg /getactivescheme 2>$null)
} | ConvertTo-Json -Depth 4 -Compress
`;

export async function machineInfo(ps, { log = null } = {}) {
  const result = await run(ps.path, ['-NoProfile', '-NonInteractive', '-Command', MACHINE_SCRIPT], {
    timeoutMs: 120000,
    log,
    label: 'machine-info',
  });
  let psInfo = {};
  if (result.code === 0) {
    try {
      psInfo = JSON.parse(result.stdout.trim());
    } catch {
      psInfo = { error: 'JSON machine invalide', raw: result.stdout.slice(0, 500) };
    }
  } else {
    psInfo = { error: result.stderr.slice(-500) };
  }
  const versions = {};
  for (const [name, cmd, args] of [
    ['node', process.execPath, ['--version']],
    ['cargo', 'cargo', ['--version']],
    ['rustc', 'rustc', ['--version']],
    ['git', 'git', ['--version']],
  ]) {
    const res = await run(cmd, args, { timeoutMs: 60000 });
    versions[name] = res.code === 0 ? res.stdout.trim() : `<indisponible: ${res.code}>`;
  }
  let disk = null;
  try {
    const stat = fs.statfsSync(process.cwd());
    disk = {
      free_bytes: stat.bfree * stat.bsize,
      available_bytes: stat.bavail * stat.bsize,
      total_bytes: stat.blocks * stat.bsize,
    };
  } catch {
    disk = null;
  }
  return {
    ...psInfo,
    logical_cpus_os: os.cpus().length,
    versions,
    disk,
    node_platform: process.platform,
    node_arch: process.arch,
  };
}

/** Instantané bon marché : RAM libre + temps CPU cumulés (pas de sous-processus). */
export function lightHostSample() {
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

/** Charge CPU moyenne (%) entre deux instantanés légers. */
export function cpuBusyPct(before, after) {
  const total = after.cpu_total_ticks - before.cpu_total_ticks;
  const idle = after.cpu_idle_ticks - before.cpu_idle_ticks;
  if (total <= 0) return null;
  return round((1 - idle / total) * 100, 1);
}
