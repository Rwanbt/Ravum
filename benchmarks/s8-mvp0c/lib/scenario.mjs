//! Scénarios S8 MVP-0c : A (copie par agent + intégration), B (répertoire
//! partagé, écritures concurrentes + détection de conflits), C (séquentiel).
//!
//! Aucune commande Git mutante n'est exécutée : la stratégie A utilise des
//! copies de répertoire et une fusion 3-way en mémoire, conformément à la
//! contrainte de la mission (le Lead commite).

import fs from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { AGENTS, TASKS, referenceContents, tasksOf } from './tasks.mjs';
import { copyTree, dirBytes, ensureDir, readContents, rmrf, round, sha256Hex, sha256File, sleep, writeContents } from './fsutil.mjs';
import { CommandLog, Tracker, parseCargoTest, run } from './exec.mjs';
import { ProcessSampler } from './sampler.mjs';
import { cpuBusyPct, lightSample, startMachineWatch } from './env.mjs';
import { integrateAgentResults, markerCheck, protectedCheck, referenceCheck } from './verify.mjs';
import { readReceipt } from './receipt.mjs';

const AGENT_SCRIPT = 'lib/agent-scripted.mjs';

function baseHashesForAgent(baseContents, agentId) {
  const hashes = {};
  for (const task of tasksOf(agentId)) hashes[task.path] = sha256Hex(baseContents[task.path]);
  return hashes;
}

async function runAgent({ ctx, agent, workspace, strategy, receiptPath, basePath, extraArgs = [], tracker, log }) {
  const calledAt = performance.now();
  const taskCsv = tasksOf(agent).map((task) => task.id).join(',');
  // Agent-command PLUGGABLE : soit l'éditeur scripté du pilote, soit une
  // commande externe fournie par l'appelant (`--agent-command`) avec les
  // substitutions {agent} {workspace} {tasks} {strategy} {base} {receipt}.
  if (ctx.agentCommand.kind === 'custom') {
    const expanded = String(ctx.agentCommand.template ?? '')
      .split('{agent}').join(agent)
      .split('{workspace}').join(workspace)
      .split('{tasks}').join(taskCsv)
      .split('{strategy}').join(strategy)
      .split('{base}').join(basePath)
      .split('{receipt}').join(receiptPath);
    if (!expanded.trim()) throw new Error('--agent-command vide');
    const command = process.platform === 'win32' ? process.env.ComSpec || 'cmd.exe' : 'sh';
    const commandArgs = process.platform === 'win32' ? ['/d', '/s', '/c', expanded] : ['-c', expanded];
    const result = await run(command, commandArgs, {
      cwd: ctx.repoRoot,
      env: ctx.cargoEnv,
      timeoutMs: 300000,
      log,
      tracker,
      label: `agent-${agent}`,
      sanitize: ctx.sanitize,
    });
    return { ...result, calledAt, doneAt: performance.now() };
  }
  const args = [
    path.join(ctx.repoRoot, 'benchmarks', 's8-mvp0c', AGENT_SCRIPT),
    '--agent',
    agent,
    '--workspace',
    workspace,
    '--tasks',
    tasksOf(agent).map((task) => task.id).join(','),
    '--strategy',
    strategy,
    '--base',
    basePath,
    '--receipt',
    receiptPath,
    ...extraArgs,
  ];
  const result = await run(process.execPath, args, {
    cwd: ctx.repoRoot,
    env: ctx.cargoEnv,
    timeoutMs: 300000,
    log,
    tracker,
    label: `agent-${agent}`,
    sanitize: ctx.sanitize,
  });
  return { ...result, calledAt, doneAt: performance.now() };
}

function readAgentFinalContents(dir, ctx) {
  return readContents(dir, ctx.editableFiles);
}

async function runCargoTest({ ctx, dir, tracker, log }) {
  const started = performance.now();
  const result = await run('cargo', ['test', '--quiet'], {
    cwd: dir,
    env: ctx.cargoEnv,
    timeoutMs: 900000,
    maxOut: 1024 * 1024,
    log,
    tracker,
    label: 'cargo-test',
    sanitize: ctx.sanitize,
  });
  return { ...parseCargoTest(result.stdout, result.stderr, result.code), ms: round(performance.now() - started), timed_out: result.timedOut, stdout_tail: result.stdout.slice(-4000), stderr_tail: result.stderr.slice(-4000) };
}

function receiptsOf(ctx, receiptsDir, agentResults) {
  return agentResults.map((agentResult) => {
    const receipt = readReceipt(path.join(receiptsDir, `${agentResult.agent}.json`));
    return { ...agentResult, receipt };
  });
}

function collectAgentMetrics(ctx, agentResults) {
  const agents = agentResults.map((agentResult) => {
    const receipt = agentResult.receipt ?? {};
    return {
      agent: agentResult.agent,
      tasks: tasksOf(agentResult.agent).map((task) => task.id),
      edit_ms: round(receipt.edit_ms ?? agentResult.editMs),
      attempts: receipt.attempts ?? null,
      conflicts: receipt.conflicts ?? 0,
      blocked_ms: round(receipt.blocked_ms ?? 0),
      self_rss_mb: receipt.agent_self_rss_bytes ? round(receipt.agent_self_rss_bytes / (1024 * 1024), 1) : null,
      status: receipt.status ?? 'missing',
      command_exit_code: agentResult.code,
      tasks_detail: receipt.tasks ?? [],
    };
  });
  return agents;
}

/** Lance une répétition d'un scénario et renvoie toutes les métriques brutes. */
export async function runScenario({ scenario, rep, scheduleIndex, ctx }) {
  const repId = `${scenario}${rep}`;
  const scenarioDir = path.join(ctx.workRoot, repId);
  ensureDir(scenarioDir);
  const log = new CommandLog();
  const sampler = new ProcessSampler({ dir: path.join(scenarioDir, 'sampler'), psPath: ctx.ps.path, scriptPath: ctx.samplerScript, intervalMs: 250 });
  const tracker = new Tracker(sampler);
  const receiptsDir = path.join(scenarioDir, 'receipts');
  ensureDir(receiptsDir);
  const baseDir = path.join(scenarioDir, 'base');
  copyTree(ctx.fixtureDir, baseDir);
  const baseContents = readContents(baseDir, ctx.fixtureFiles);
  const reference = referenceContents(baseContents);
  const receiptBases = {};
  for (const agent of AGENTS) {
    const hashes = baseHashesForAgent(baseContents, agent.id);
    const file = path.join(receiptsDir, `${agent.id}.base.json`);
    fs.writeFileSync(file, `${JSON.stringify(hashes)}\n`, 'utf8');
    receiptBases[agent.id] = hashes;
  }

  let samplerResult = null;
  const hostBefore = lightSample();
  const startedEpochMs = Date.now();
  const machineWatch = startMachineWatch({ intervalMs: 150 });
  sampler.start();
  await sampler.waitForReady({ timeoutMs: 12000 });
  await sleep(100);

  try {
    const t0 = performance.now();
    let finalDir = null;
    let agentResults = [];
    let integration = { conflicts: [], merge_ms: 0, merges: 0 };
    let editPhaseMs = 0;
    let integrationPhaseMs = 0;
    let agentWaits = {};
    let extra = {};

    if (scenario === 'A') {
      // Une copie par agent, puis intégration 3-way en mémoire.
      const promises = AGENTS.map(async (agent) => {
        const dir = path.join(scenarioDir, 'agents', agent.id);
        copyTree(baseDir, dir);
        const result = await runAgent({
          ctx,
          agent: agent.id,
          workspace: dir,
          strategy: 'A',
          receiptPath: path.join(receiptsDir, `${agent.id}.json`),
          basePath: path.join(receiptsDir, `${agent.id}.base.json`),
          tracker,
          log,
        });
        return { agent: agent.id, dir, result, editMs: round(performance.now() - t0) };
      });
      const raw = await Promise.all(promises);
      editPhaseMs = round(performance.now() - t0);
      agentResults = receiptsOf(
        ctx,
        receiptsDir,
        raw.map((entry) => ({ agent: entry.agent, dir: entry.dir, code: entry.result.code, editMs: entry.editMs, doneAt: entry.result.doneAt })),
      );
      const finals = agentResults.map((entry) => ({ agent: entry.agent, finalContents: readAgentFinalContents(entry.dir, ctx) }));
      const integrationStart = performance.now();
      const integrated = integrateAgentResults({ baseContents, agentResults: finals });
      const integrationDir = path.join(scenarioDir, 'integration');
      copyTree(baseDir, integrationDir);
      writeContents(integrationDir, integrated.contents);
      integrationPhaseMs = round(performance.now() - integrationStart);
      integration = { conflicts: integrated.conflicts, merge_ms: round(integrated.merge_ms), merges: integrated.merges };
      finalDir = integrationDir;
      const integratedAt = round(performance.now() - t0);
      agentWaits = Object.fromEntries(agentResults.map((entry) => [entry.agent, round(Math.max(0, integratedAt - (entry.doneAt - t0)))]));
    } else if (scenario === 'B') {
      // Un seul répertoire partagé ; écritures concurrentes.
      const sharedDir = path.join(scenarioDir, 'shared');
      copyTree(baseDir, sharedDir);
      const staggerMs = ctx.pilot.collisionStaggerMs;
      const delayMs = ctx.pilot.collisionDelayMs;
      const order = ['agent-2', 'agent-1', 'agent-3'];
      const promises = order.map((agentId, index) =>
        (async () => {
          if (index > 0) await sleep(staggerMs);
          const extraArgs = agentId === 'agent-2' ? ['--collision-task', 't3', '--collision-delay-ms', String(delayMs)] : [];
          const result = await runAgent({
            ctx,
            agent: agentId,
            workspace: sharedDir,
            strategy: 'B',
            receiptPath: path.join(receiptsDir, `${agentId}.json`),
            basePath: path.join(receiptsDir, `${agentId}.base.json`),
            extraArgs,
            tracker,
            log,
          });
          return { agent: agentId, dir: sharedDir, result, editMs: round(performance.now() - t0) };
        })(),
      );
      const raw = await Promise.all(promises);
      editPhaseMs = round(performance.now() - t0);
      agentResults = receiptsOf(
        ctx,
        receiptsDir,
        raw.map((entry) => ({ agent: entry.agent, dir: entry.dir, code: entry.result.code, editMs: entry.editMs, doneAt: entry.result.doneAt })),
      );
      agentWaits = Object.fromEntries(agentResults.map((entry) => [entry.agent, round(entry.receipt?.blocked_ms ?? 0)]));
      finalDir = sharedDir;
      extra = { collision: { injected: true, task: 't3', delay_ms: delayMs, stagger_ms: staggerMs, note: 'fenêtre de collision déterministe du pilote pour exercer la détection' } };
    } else if (scenario === 'C') {
      // Séquentiel strict : un agent après l'autre, branche/répertoire unique.
      const sharedDir = path.join(scenarioDir, 'shared');
      copyTree(baseDir, sharedDir);
      const waits = {};
      for (const agent of AGENTS) {
        const startAt = performance.now();
        waits[agent.id] = round(startAt - t0);
        const result = await runAgent({
          ctx,
          agent: agent.id,
          workspace: sharedDir,
          strategy: 'C',
          receiptPath: path.join(receiptsDir, `${agent.id}.json`),
          basePath: path.join(receiptsDir, `${agent.id}.base.json`),
          tracker,
          log,
        });
        agentResults.push({ agent: agent.id, dir: sharedDir, code: result.code, editMs: round(performance.now() - t0), doneAt: result.doneAt });
        agentResults[agentResults.length - 1].receipt = readReceipt(path.join(receiptsDir, `${agent.id}.json`));
      }
      editPhaseMs = round(performance.now() - t0);
      agentWaits = waits;
      finalDir = sharedDir;
      agentResults = agentResults.map((entry) => ({ ...entry, receipt: entry.receipt ?? readReceipt(path.join(receiptsDir, `${entry.agent}.json`)) }));
    } else {
      throw new Error(`scénario inconnu: ${scenario}`);
    }

    const agents = collectAgentMetrics(ctx, agentResults);
    const finalContents = readContents(finalDir, ctx.fixtureFiles);
    const content = referenceCheck(finalContents, reference);
    const markers = markerCheck(finalContents);
    const protectedChanged = protectedCheck(finalContents, baseContents);
    const lostTasks = TASKS.filter((task) => !markers[task.id].present).map((task) => task.id);
    const missingReceipts = agentResults.filter((entry) => !entry.receipt).map((entry) => entry.agent);
    const tests = await runCargoTest({ ctx, dir: finalDir, tracker, log });
    const tTotalMs = round(performance.now() - t0);
    const hostAfter = lightSample();
    const machine = machineWatch.stop();
    samplerResult = await sampler.stop();

    const targetDir = path.join(finalDir, 'target');
    const sizes = {
      strategy_dir_bytes: dirBytes(finalDir),
      target_bytes: fs.existsSync(targetDir) ? dirBytes(targetDir) : 0,
      receipts_dir_bytes: dirBytes(receiptsDir),
    };

    const agentWaitTotal = round(Object.values(agentWaits).reduce((acc, value) => acc + value, 0));
    const conflictsCount = scenario === 'A' ? integration.conflicts.length : agents.reduce((acc, agent) => acc + (agent.conflicts ?? 0), 0);
    const conflictResolutionMs = scenario === 'A' ? integration.merge_ms : round(agents.reduce((acc, agent) => acc + (agent.blocked_ms ?? 0), 0));
    const writes = scenario === 'A' ? agentResults.reduce((acc, entry) => acc + (entry.receipt?.changed?.filter((file) => file.changed_vs_base).length ?? 0), 0) + integration.merges : agents.reduce((acc, agent) => acc + (agent.attempts ?? 0), 0);
    const builds = log.entries.filter((entry) => entry.label === 'cargo-test').length;

    const result = {
      rep_id: repId,
      scenario,
      rep,
      schedule_index: scheduleIndex,
      ok: true,
      timings: {
        t_total_ms: tTotalMs,
        edit_phase_ms: editPhaseMs,
        integration_phase_ms: integrationPhaseMs,
        test_ms: tests.ms,
        setup_ms: null,
      },
      agents,
      agent_waits_ms: agentWaits,
      agent_wait_total_ms: agentWaitTotal,
      conflicts: {
        count: conflictsCount,
        resolution_ms: conflictResolutionMs,
        detail: scenario === 'A' ? integration.conflicts : agents.flatMap((agent) => (agent.conflicts > 0 ? [{ agent: agent.agent, task: 't3', detected: 'version optimiste (relecture)' }] : [])),
      },
      writes: {
        kind: scenario === 'A' ? 'copies + intégration 3-way' : scenario === 'B' ? 'répertoire partagé' : 'séquentiel',
        attempts: writes,
        merges: scenario === 'A' ? integration.merges : 0,
      },
      builds,
      iterations: agents.reduce((acc, agent) => acc + (agent.attempts ?? 0), 0),
      tests,
      tokens: { measured: false, reason: 'pilote avec éditeurs SCRIPTÉS (aucun LLM, aucune session DSH)' },
      human_supervision: { measured: false, reason: 'pilote scripté ; supervision humaine non mesurée' },
      lost: {
        tasks_missing_markers: lostTasks,
        content_mismatches: content.mismatches,
        protected_files_changed: protectedChanged,
        missing_receipts: missingReceipts,
      },
      verification: {
        content_ok: content.ok,
        markers,
        protected_ok: protectedChanged.length === 0,
        receipts_ok: missingReceipts.length === 0,
      },
      resources: {
        sampler: samplerResult,
        machine,
        host_before: hostBefore,
        host_after: hostAfter,
        cpu_busy_pct_over_rep: cpuBusyPct(hostBefore, hostAfter),
      },
      sizes,
      extra,
      commands: log.entries,
      commands_dropped: log.dropped,
      definitions: {
        t_total_ms: 'départ des agents → fin de cargo test vert (wall-clock)',
        edit_phase_ms: scenario === 'C' ? 'somme des éditions séquentielles' : 'phase d’édition concurrente des 3 agents',
        integration_phase_ms: 'A : fusion 3-way en mémoire + écriture de la copie d’intégration ; B/C : 0',
        agent_wait_ms: scenario === 'A' ? 'fin d’édition → intégration terminée' : scenario === 'B' ? 'temps bloqué par une collision détectée (relecture + réessai)' : 'début de campagne → début de l’agent (attente des prédécesseurs)',
        conflicts: 'A : hunks en conflit lors de la fusion ; B : collisions détectées par garde de version ; C : 0',
        iterations: 'nombre total de tentatives d’écriture des agents (réessais inclus)',
      },
    };
    result.functional_success = tests.green && content.ok && lostTasks.length === 0 && protectedChanged.length === 0 && missingReceipts.length === 0;
    return result;
  } finally {
    if (!samplerResult) {
      try { await sampler.stop({ timeoutMs: 4000 }); } catch { /* ignore */ }
    }
    if (!ctx.keep) {
      try { rmrf(scenarioDir); } catch { /* nettoyage best-effort */ }
    }
  }
}
