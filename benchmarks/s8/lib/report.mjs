//! Agrégation, tableaux et écriture des preuves du benchmark S8.

import fs from 'node:fs';
import path from 'node:path';
import { describe, ensureDir, round, sum, writeJsonAtomic } from './util.mjs';

export const SCENARIO_LABELS = {
  A: 'A — un worktree Git par agent',
  B: 'B — workspace partagé + coordinateur Ravum',
  C: 'C — agents séquentiels (contrôle)',
};

export const SCENARIO_SHORT = {
  A: 'A worktrees',
  B: 'B coordinateur',
  C: 'C séquentiel',
};

/** Ordre alterné : bloc croissant A,B,C puis bloc décroissant C,B,A, etc. */
export function buildSchedule(reps, scenarios) {
  const list = [];
  for (let rep = 1; rep <= reps; rep += 1) {
    const order = rep % 2 === 1 ? [...scenarios] : [...scenarios].reverse();
    for (const scenario of order) list.push({ scenario, rep });
  }
  return list;
}

function writesOf(rep) {
  if (rep.scenario === 'A') return rep.writes.merge_commits;
  if (rep.scenario === 'B') return rep.writes.publication_commits;
  return rep.writes.commits;
}

/** Métriques comparatives (les timings sont en millisecondes wall-clock). */
export const METRICS = [
  { key: 't_total_ms', label: 'Temps total jusqu’à tests verts (ms)', get: (r) => r.timings.t_total_ms },
  { key: 'edit_phase_ms', label: 'Phase d’édition des agents (ms)', get: (r) => r.timings.edit_phase_ms },
  { key: 'integration_phase_ms', label: 'Phase d’intégration/publication (ms)', get: (r) => r.timings.integration_phase_ms },
  { key: 'test_ms', label: 'cargo test final (ms)', get: (r) => r.timings.test_ms },
  { key: 'agent_wait_total_ms', label: 'Attente cumulée des agents (ms)', get: (r) => r.agent_wait_total_ms },
  { key: 'conflicts', label: 'Conflits détectés', get: (r) => r.conflicts.count },
  { key: 'conflict_resolution_ms', label: 'Temps de résolution des conflits (ms)', get: (r) => r.conflicts.resolution_ms },
  { key: 'writes', label: 'Merges / publications / commits', get: (r) => writesOf(r) },
  { key: 'rebuilds', label: 'Rebuilds (invocations cargo)', get: (r) => r.rebuilds ?? 1 },
  { key: 'tests_passed', label: 'Tests verts (sur 10)', get: (r) => r.tests.passed },
  { key: 'lost_accepted', label: 'Changements acceptés perdus', get: (r) => r.lost.accepted_missing.length },
  { key: 'lost_independent', label: 'Éditions indépendantes perdues', get: (r) => r.lost.independent_missing.length },
  { key: 'silent_overwrites', label: 'Écrasements silencieux', get: (r) => r.lost.silent_overwrites },
  { key: 'ram_peak_mb', label: 'RAM pic suivie totale (Mo WorkingSet)', get: (r) => r.resources.sampler.peak_ws_mb },
  { key: 'ram_coordinator_mb', label: 'RAM pic coordinateur ravum-collab (Mo)', get: (r) => r.resources.sampler.families?.coordinator?.peak_ws_mb ?? null },
  { key: 'ram_testers_mb', label: 'RAM pic testeurs cargo/rustc/test (Mo)', get: (r) => r.resources.sampler.families?.testers?.peak_ws_mb ?? null },
  { key: 'cpu_seconds', label: 'CPU consommé par la campagne (s)', get: (r) => r.resources.sampler.cpu_seconds },
  { key: 'cpu_busy_pct', label: 'Charge CPU machine pendant la répétition (%)', get: (r) => r.resources.cpu_busy_pct_over_rep },
  { key: 'cache_target_mb', label: 'Cache target/ de la réplique (Mo)', get: (r) => round(r.sizes.target_bytes / (1024 * 1024), 2) },
  { key: 'cache_git_mb', label: 'Objets .git/ de la réplique (Mo)', get: (r) => round(r.sizes.git_bytes / (1024 * 1024), 2) },
  { key: 'disk_repo_mb', label: 'Espace disque de la réplique (Mo)', get: (r) => round(r.sizes.repo_bytes / (1024 * 1024), 2) },
  { key: 'functional_success', label: 'Réussite fonctionnelle stricte (0/1)', get: (r) => (r.functional_success ? 1 : 0) },
];

export function aggregateScenario(reps) {
  const metrics = {};
  for (const metric of METRICS) {
    const values = reps.map((rep) => Number(metric.get(rep))).filter((value) => Number.isFinite(value));
    metrics[metric.key] = { label: metric.label, ...describe(values) };
  }
  return {
    reps: reps.length,
    metrics,
    all_success: reps.every((rep) => rep.functional_success),
    all_tests_green: reps.every((rep) => rep.tests.green),
    all_content_ok: reps.every((rep) => rep.verification.content_ok),
  };
}

function fmt(value) {
  if (value === null || value === undefined || !Number.isFinite(value)) return '—';
  if (Number.isInteger(value)) return String(value);
  return String(round(value, 2));
}

/** Tableau comparatif Markdown A/B/C : médiane [min–max] (IQR). */
export function markdownTable(summary) {
  const scenarios = Object.keys(summary.scenarios);
  const lines = [];
  lines.push(`| Métrique | ${scenarios.map((s) => SCENARIO_SHORT[s]).join(' | ')} |`);
  lines.push(`|---|${scenarios.map(() => '---').join('|')}|`);
  for (const metric of METRICS) {
    const cells = scenarios.map((scenario) => {
      const stats = summary.scenarios[scenario].metrics[metric.key];
      if (!stats || stats.n === 0) return '—';
      if (stats.n === 1) return fmt(stats.median);
      return `${fmt(stats.median)} [${fmt(stats.min)}–${fmt(stats.max)}] (IQR ${fmt(stats.iqr)})`;
    });
    lines.push(`| ${metric.label} | ${cells.join(' | ')} |`);
  }
  return lines.join('\n');
}

/** Tableau brut par répétition (une ligne par (scénario, rep)). */
export function rawTable(reps) {
  const cols = [
    ['rep_id', (r) => r.rep_id],
    ['ordre', (r) => r.schedule_index],
    ['t_total_ms', (r) => r.timings.t_total_ms],
    ['edit_ms', (r) => r.timings.edit_phase_ms],
    ['integration_ms', (r) => r.timings.integration_phase_ms],
    ['test_ms', (r) => r.timings.test_ms],
    ['attente_agents_ms', (r) => r.agent_wait_total_ms],
    ['conflits', (r) => r.conflicts.count],
    ['writes', (r) => writesOf(r)],
    ['tests', (r) => `${r.tests.passed ?? '?'}/10`],
    ['perdus', (r) => r.lost.accepted_missing.length],
    ['écrasés', (r) => r.lost.silent_overwrites],
    ['RAM_totale_Mo', (r) => r.resources.sampler.peak_ws_mb],
    ['RAM_testeurs_Mo', (r) => r.resources.sampler.families?.testers?.peak_ws_mb ?? 0],
    ['RAM_coord_Mo', (r) => r.resources.sampler.families?.coordinator?.peak_ws_mb ?? 0],
    ['CPU_s', (r) => r.resources.sampler.cpu_seconds],
    ['target_Mo', (r) => round(r.sizes.target_bytes / (1024 * 1024), 2)],
    ['succès', (r) => (r.functional_success ? 1 : 0)],
  ];
  const lines = [];
  lines.push(`| ${cols.map(([name]) => name).join(' | ')} |`);
  lines.push(`|${cols.map(() => '---').join('|')}|`);
  for (const rep of reps) lines.push(`| ${cols.map(([, get]) => String(get(rep))).join(' | ')} |`);
  return lines.join('\n');
}

/** Écrit les preuves JSON : un fichier par scénario + un résumé global. */
export function writeEvidence({ evidenceDir, dateStamp, environment, schedule, reps, scenarioSummaries, validation, loadProbes, coordinatorInfo, markdown, rawMarkdown }) {
  ensureDir(evidenceDir);
  const files = {};
  for (const scenario of Object.keys(scenarioSummaries)) {
    const scenarioReps = reps.filter((rep) => rep.scenario === scenario);
    const payload = {
      schema_version: 1,
      scenario,
      scenario_label: SCENARIO_LABELS[scenario],
      generated_at: new Date().toISOString(),
      environment,
      coordinator_build: coordinatorInfo,
      schedule,
      scenario_reps: schedule.filter((item) => item.scenario === scenario),
      load_probes: loadProbes,
      summary: scenarioSummaries[scenario],
      reps: scenarioReps,
    };
    const file = path.join(evidenceDir, `mvp0b-s8-${scenario}-${dateStamp}.json`);
    writeJsonAtomic(file, payload);
    files[scenario] = file;
  }
  const summaryPayload = {
    schema_version: 1,
    benchmark: 'S8 — worktrees vs workspace partagé vs séquentiel',
    generated_at: new Date().toISOString(),
    environment,
    coordinator_build: coordinatorInfo,
    schedule,
    load_probes: loadProbes,
    scenario_summaries: scenarioSummaries,
    validation,
    markdown,
    raw_markdown: rawMarkdown,
    evidence_files: files,
  };
  const summaryFile = path.join(evidenceDir, `mvp0b-s8-summary-${dateStamp}.json`);
  writeJsonAtomic(summaryFile, summaryPayload);
  return { files, summaryFile };
}

/** Écrit un rapport Markdown lisible (regénéré à chaque exécution). */
export function writeMarkdownReport({ file, summary, markdown, rawMarkdown, environment, validation, loadProbes }) {
  ensureDir(path.dirname(file));
  const lines = [];
  lines.push('# S8 — Résultats bruts (généré automatiquement)');
  lines.push('');
  lines.push(`Généré le ${new Date().toISOString()}.`);
  lines.push('');
  lines.push('IMPORTANT : les « agents » sont des éditeurs **scriptés déterministes**, pas des LLM.');
  lines.push('Les tokens IA et le temps de supervision humaine ne sont pas mesurés (ils n’existent pas dans ce harnais).');
  lines.push('');
  lines.push('## Environnement');
  lines.push('');
  lines.push('```json');
  lines.push(JSON.stringify(environment, null, 2));
  lines.push('```');
  lines.push('');
  lines.push('## Charge machine (sondes complètes)');
  lines.push('');
  lines.push('```json');
  lines.push(JSON.stringify(loadProbes, null, 2));
  lines.push('```');
  lines.push('');
  lines.push('## Tableau comparatif (médiane [min–max] (IQR), n par scénario)');
  lines.push('');
  lines.push(markdown);
  lines.push('');
  lines.push('## Échantillons bruts');
  lines.push('');
  lines.push(rawMarkdown);
  lines.push('');
  lines.push('## Validation du protocole');
  lines.push('');
  lines.push('```json');
  lines.push(JSON.stringify(validation, null, 2));
  lines.push('```');
  lines.push('');
  fs.writeFileSync(file, `${lines.join('\n')}\n`, 'utf8');
  return file;
}

export function consoleTable(summary) {
  return markdownTable(summary);
}

export { sum };
