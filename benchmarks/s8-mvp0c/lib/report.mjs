//! Agrégation et preuves du pilote S8 MVP-0c.

import fs from 'node:fs';
import path from 'node:path';
import { describe, ensureDir, round, writeJsonAtomic } from './fsutil.mjs';

export const SCENARIO_LABELS = {
  A: 'A — copie/worktree par agent puis intégration',
  B: 'B — répertoire partagé, écritures concurrentes',
  C: 'C — séquentiel sur un répertoire unique',
};

export const METRICS = [
  { key: 't_total_ms', label: 'Temps total jusqu’à tests verts (ms)', get: (rep) => rep.timings.t_total_ms },
  { key: 'edit_phase_ms', label: 'Phase d’édition des agents (ms)', get: (rep) => rep.timings.edit_phase_ms },
  { key: 'integration_phase_ms', label: 'Phase d’intégration (ms)', get: (rep) => rep.timings.integration_phase_ms },
  { key: 'test_ms', label: 'cargo test final (ms)', get: (rep) => rep.timings.test_ms },
  { key: 'agent_wait_total_ms', label: 'Attente cumulée des agents (ms)', get: (rep) => rep.agent_wait_total_ms },
  { key: 'conflicts', label: 'Conflits détectés', get: (rep) => rep.conflicts.count },
  { key: 'conflict_resolution_ms', label: 'Temps de résolution des conflits (ms)', get: (rep) => rep.conflicts.resolution_ms },
  { key: 'merges', label: 'Fusions / intégrations', get: (rep) => rep.writes.merges },
  { key: 'writes', label: 'Écritures / tentatives', get: (rep) => rep.writes.attempts },
  { key: 'iterations', label: 'Itérations d’agents (tentatives)', get: (rep) => rep.iterations },
  { key: 'builds', label: 'Compilations cargo', get: (rep) => rep.builds },
  { key: 'tests_passed', label: 'Tests verts (sur 12)', get: (rep) => rep.tests.passed },
  { key: 'lost_changes', label: 'Changements perdus', get: (rep) => rep.lost.tasks_missing_markers.length + (rep.verification.content_ok ? 0 : 1) },
  { key: 'cpu_seconds', label: 'CPU des processus suivis (s)', get: (rep) => rep.resources.sampler.cpu_seconds },
  { key: 'ram_peak_mb', label: 'RAM pic des processus suivis (Mo WorkingSet)', get: (rep) => rep.resources.sampler.peak_ws_mb },
  { key: 'agent_max_rss_mb', label: 'RAM des agents (RSS auto-déclaré, Mo)', get: (rep) => (rep.agents && rep.agents.length > 0 ? Math.max(...rep.agents.map((agent) => agent.self_rss_mb ?? 0)) : null) },
  { key: 'machine_peak_cpu_pct', label: 'Charge CPU machine pendant la répétition (%)', get: (rep) => rep.resources.machine?.machine_peak_cpu_busy_pct ?? null },
  { key: 'machine_min_free_ram_mb', label: 'RAM machine libre minimale (Mo)', get: (rep) => (rep.resources.machine?.machine_min_free_ram_bytes === null || rep.resources.machine?.machine_min_free_ram_bytes === undefined ? null : Math.round(rep.resources.machine.machine_min_free_ram_bytes / (1024 * 1024))) },
  { key: 'machine_ram_drop_mb', label: 'Chute RAM machine pendant la répétition (Mo)', get: (rep) => (rep.resources.machine?.machine_ram_drop_bytes === null || rep.resources.machine?.machine_ram_drop_bytes === undefined ? null : Math.round(rep.resources.machine.machine_ram_drop_bytes / (1024 * 1024))) },
  { key: 'disk_target_mb', label: 'Cache target/ de la réplique (Mo)', get: (rep) => round(rep.sizes.target_bytes / (1024 * 1024), 2) },
  { key: 'disk_workspace_mb', label: 'Disque de la réplique (Mo)', get: (rep) => round(rep.sizes.strategy_dir_bytes / (1024 * 1024), 2) },
  { key: 'functional_success', label: 'Réussite fonctionnelle (0/1)', get: (rep) => (rep.functional_success ? 1 : 0) },
];

export function buildSchedule(reps, scenarios = ['A', 'B', 'C']) {
  const list = [];
  for (let rep = 1; rep <= reps; rep += 1) {
    const order = rep % 2 === 1 ? [...scenarios] : [...scenarios].reverse();
    for (const scenario of order) list.push({ scenario, rep });
  }
  return list;
}

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
    conflicts_total: reps.reduce((acc, rep) => acc + rep.conflicts.count, 0),
    lost_total: reps.reduce((acc, rep) => acc + rep.lost.tasks_missing_markers.length, 0),
  };
}

function fmt(value) {
  if (value === null || value === undefined || !Number.isFinite(value)) return '—';
  if (Number.isInteger(value)) return String(value);
  return String(round(value, 2));
}

export function markdownTable(summary) {
  const scenarios = Object.keys(summary.scenarios);
  const lines = [];
  lines.push(`| Métrique | ${scenarios.map((scenario) => SCENARIO_LABELS[scenario].split(' — ')[0]).join(' | ')} |`);
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

export function rawTable(reps) {
  const cols = [
    ['rep', (rep) => rep.rep_id],
    ['ordre', (rep) => rep.schedule_index],
    ['total_ms', (rep) => rep.timings.t_total_ms],
    ['edit_ms', (rep) => rep.timings.edit_phase_ms],
    ['integr_ms', (rep) => rep.timings.integration_phase_ms],
    ['test_ms', (rep) => rep.timings.test_ms],
    ['attente_ms', (rep) => rep.agent_wait_total_ms],
    ['conflits', (rep) => rep.conflicts.count],
    ['résol_ms', (rep) => rep.conflicts.resolution_ms],
    ['itérations', (rep) => rep.iterations],
    ['tests', (rep) => `${rep.tests.passed}/12`],
    ['perdus', (rep) => rep.lost.tasks_missing_markers.length],
    ['RAM_Mo', (rep) => rep.resources.sampler.peak_ws_mb],
    ['CPU_s', (rep) => rep.resources.sampler.cpu_seconds],
    ['target_Mo', (rep) => round(rep.sizes.target_bytes / (1024 * 1024), 2)],
    ['succès', (rep) => (rep.functional_success ? 1 : 0)],
  ];
  const lines = [];
  lines.push(`| ${cols.map(([name]) => name).join(' | ')} |`);
  lines.push(`|${cols.map(() => '---').join('|')}|`);
  for (const rep of reps) lines.push(`| ${cols.map(([, get]) => String(get(rep))).join(' | ')} |`);
  return lines.join('\n');
}

export function writeEvidence({ file, payload }) {
  ensureDir(path.dirname(file));
  writeJsonAtomic(file, payload);
  return file;
}

export function writeMarkdownReport({ file, payload, markdown, rawMarkdown }) {
  ensureDir(path.dirname(file));
  const lines = [];
  lines.push('# S8 MVP-0c — pilote scripté : résultats bruts (généré automatiquement)');
  lines.push('');
  lines.push(`Généré le ${payload.generated_at}. Mode : **${payload.mode}**.`);
  lines.push('');
  lines.push('> Les agents du pilote sont des **éditeurs scriptés déterministes**, pas des LLM :');
  lines.push('> les tokens IA ne sont pas mesurés (aucune session DSH produite) et le temps de');
  lines.push('> supervision humaine n’est pas mesuré. Aucun chiffre n’est inventé.');
  lines.push('');
  lines.push('## Environnement');
  lines.push('');
  lines.push('```json');
  lines.push(JSON.stringify(payload.environment, null, 2));
  lines.push('```');
  lines.push('');
  lines.push('## Fixture (tests rouges avant éditions)');
  lines.push('');
  lines.push('```json');
  lines.push(JSON.stringify(payload.fixture.base_red_check, null, 2));
  lines.push('```');
  lines.push('');
  lines.push('## Sonde de détection de conflits (contrôle positif)');
  lines.push('');
  lines.push('```json');
  lines.push(JSON.stringify(payload.positive_control, null, 2));
  lines.push('```');
  lines.push('');
  lines.push('## Tableau comparatif (médiane [min–max] (IQR), n = ' + payload.schedule.length / 3 + ')');
  lines.push('');
  lines.push(markdown);
  lines.push('');
  lines.push('## Échantillons bruts');
  lines.push('');
  lines.push(rawMarkdown);
  lines.push('');
  lines.push('## Tokens et supervision');
  lines.push('');
  lines.push('```json');
  lines.push(JSON.stringify({ tokens: payload.tokens, human_supervision: payload.human_supervision }, null, 2));
  lines.push('```');
  lines.push('');
  lines.push('## Validation du protocole');
  lines.push('');
  lines.push('```json');
  lines.push(JSON.stringify(payload.validation, null, 2));
  lines.push('```');
  lines.push('');
  fs.writeFileSync(file, `${lines.join('\n')}\n`, 'utf8');
  return file;
}
