//! Campagnes S8 : A (worktree par agent), B (workspace partagé + coordinateur),
//! C (agents séquentiels, contrôle).
//!
//! Chaque répétition travaille sur une copie éphémère de la fixture dans le
//! dossier temporaire du run. Rien n'est écrit dans le dépôt Ravum lui-même :
//! les commits créés appartiennent aux répliques temporaires.

import fs from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { CommandLog, Tracker, run, runOk } from './exec.mjs';
import { ProcessSampler } from './sampler.mjs';
import { cpuBusyPct, lightHostSample } from './env.mjs';
import {
  AGENTS,
  TASKS,
  WINNER,
  acceptedTaskIds,
  applyOps,
  buildUnifiedPatch,
  expectedTaskOrder,
  independentTaskIds,
  markersForTask,
  referenceContents,
  tasksOf,
} from './tasks.mjs';
import { copyTree, dirBytes, listFiles, round, sha256File, sha256Hex, sleep, writeJsonAtomic } from './util.mjs';

const GIT_BASE_ENV = { ...process.env, GIT_TERMINAL_PROMPT: '0' };
const CARGO_ENV = {
  ...process.env,
  CARGO_TERM_COLOR: 'never',
  CARGO_NET_OFFLINE: 'true',
  RUST_BACKTRACE: '0',
  RUSTFLAGS: '',
};

function tail(text, max = 4000) {
  if (!text) return '';
  return text.length > max ? text.slice(text.length - max) : text;
}

// --- helpers Git -----------------------------------------------------------

async function initRepo({ fixtureDir, repoDir, log, tracker, sanitize }) {
  const t0 = performance.now();
  copyTree(fixtureDir, repoDir);
  const git = (args, extra = {}) =>
    runOk('git', args, { cwd: repoDir, env: GIT_BASE_ENV, log, tracker, label: `git-${args[0]}`, sanitize, ...extra });
  await git(['init', '-q', '-b', 'integration']);
  await git(['config', 'user.name', 'S8 Harness']);
  await git(['config', 'user.email', 's8@ravum.invalid']);
  await git(['config', 'commit.gpgsign', 'false']);
  // Line endings déterministes : aucun réécriture CRLF ne doit fausser les
  // sha256 attendus par le coordinateur ni le contenu de référence.
  await git(['config', 'core.autocrlf', 'false']);
  await git(['config', 'core.eol', 'lf']);
  await git(['config', 'core.safecrlf', 'false']);
  await git(['config', 'core.hooksPath', '.git/hooks']);
  await git(['add', '-A']);
  await git(['commit', '-q', '-m', 'chore: base fixture S8']);
  const head = (await git(['rev-parse', 'HEAD'])).stdout.trim();
  return { base_commit: head, init_ms: round(performance.now() - t0) };
}

async function gitCapture(repoDir, log, tracker) {
  const git = async (args) => {
    const res = await run('git', args, { cwd: repoDir, env: GIT_BASE_ENV, log, tracker, label: `git-${args[0]}`, timeoutMs: 120000 });
    return res.code === 0 ? res.stdout.trim() : `<échec code ${res.code}>`;
  };
  return {
    worktree_list: await git(['worktree', 'list', '--porcelain']),
    branch_list: await git(['branch', '--list']),
    head: await git(['rev-parse', 'HEAD']),
    status_porcelain: await git(['status', '--porcelain']),
    log_graph: tail(await git(['log', '--graph', '--oneline', '--all', '-n', '40']), 6000),
  };
}

async function statusPorcelain(repoDir, log, tracker) {
  const res = await run('git', ['status', '--porcelain'], { cwd: repoDir, env: GIT_BASE_ENV, log, tracker, label: 'git-status' });
  return res.code === 0 ? res.stdout.trim() : `<échec code ${res.code}>`;
}

function applyOpsToDir(dir, ops) {
  const paths = [...new Set(ops.map((op) => op.path))];
  const contents = {};
  for (const rel of paths) contents[rel] = fs.readFileSync(path.join(dir, rel), 'utf8');
  const { contents: next, applied } = applyOps(contents, ops);
  for (const rel of paths) fs.writeFileSync(path.join(dir, rel), next[rel], 'utf8');
  return applied;
}

function readContents(dir, relPaths) {
  const contents = {};
  for (const rel of relPaths) contents[rel] = fs.readFileSync(path.join(dir, rel), 'utf8');
  return contents;
}

async function runCargoTest({ repoDir, log, tracker, sanitize }) {
  const t0 = performance.now();
  const res = await run('cargo', ['test', '--quiet'], {
    cwd: repoDir,
    env: CARGO_ENV,
    log,
    tracker,
    label: 'cargo-test',
    sanitize,
    timeoutMs: 1800000,
    maxOut: 1024 * 1024,
  });
  const ms = round(performance.now() - t0);
  const combined = `${res.stdout}\n${res.stderr}`;
  const resultLines = [...combined.matchAll(/test result: (ok|FAILED)\. (\d+) passed; (\d+) failed/g)];
  let passed = 0;
  let failed = 0;
  let allOk = true;
  for (const line of resultLines) {
    passed += Number(line[2]);
    failed += Number(line[3]);
    if (line[1] !== 'ok') allOk = false;
  }
  return {
    ms,
    green: res.code === 0 && allOk && failed === 0 && passed > 0,
    passed,
    failed,
    test_targets: resultLines.length,
    exit_code: res.code,
    timed_out: res.timedOut,
    stdout_tail: tail(res.stdout, 6000),
    stderr_tail: tail(res.stderr, 6000),
  };
}

function verifyContents({ repoDir, expected, fixtureFiles }) {
  const mismatches = [];
  for (const rel of fixtureFiles) {
    const want = expected[rel];
    if (want === undefined) continue;
    const abs = path.join(repoDir, rel);
    const actual = fs.existsSync(abs) ? fs.readFileSync(abs, 'utf8') : null;
    if (actual !== want) {
      mismatches.push({
        path: rel,
        expected_sha256: sha256Hex(Buffer.from(want, 'utf8')),
        actual_sha256: actual === null ? null : sha256Hex(Buffer.from(actual, 'utf8')),
        actual_tail: actual === null ? null : tail(actual, 1200),
      });
    }
  }
  return { ok: mismatches.length === 0, mismatches };
}

function markerCheck({ repoDir, markersByTask }) {
  const cache = new Map();
  const linesOf = (rel) => {
    if (!cache.has(rel)) {
      const abs = path.join(repoDir, rel);
      cache.set(rel, fs.existsSync(abs) ? fs.readFileSync(abs, 'utf8').split('\n') : []);
    }
    return cache.get(rel);
  };
  const out = {};
  for (const [taskId, markers] of Object.entries(markersByTask)) {
    const missing = [];
    for (const marker of markers) {
      if (!linesOf(marker.path).includes(marker.line)) missing.push(marker);
    }
    out[taskId] = { present: missing.length === 0, missing };
  }
  return out;
}

function measureSizes(repoDir) {
  const result = { repo_bytes: dirBytes(repoDir), target_bytes: 0, git_bytes: 0 };
  const targetDir = path.join(repoDir, 'target');
  if (fs.existsSync(targetDir)) result.target_bytes = dirBytes(targetDir);
  const gitDir = path.join(repoDir, '.git');
  if (fs.existsSync(gitDir)) result.git_bytes = dirBytes(gitDir);
  return result;
}

function baseMarkers(baseContents) {
  const map = {};
  for (const task of TASKS) map[task.id] = markersForTask(task, baseContents);
  return map;
}

// --- scénario A ------------------------------------------------------------

async function agentEditA({ agent, worktreeDir, log, tracker, sanitize, campaignStart }) {
  const started = performance.now();
  const tasks = [];
  for (const task of tasksOf(agent)) {
    const t0 = performance.now();
    applyOpsToDir(worktreeDir, task.ops);
    await runOk('git', ['add', '-A'], { cwd: worktreeDir, env: GIT_BASE_ENV, log, tracker, label: `git-add-${agent}`, sanitize });
    await runOk('git', ['commit', '-q', '-m', `${task.id}: ${task.title}`], {
      cwd: worktreeDir,
      env: GIT_BASE_ENV,
      log,
      tracker,
      label: `git-commit-${agent}`,
      sanitize,
    });
    tasks.push({ id: task.id, ms: round(performance.now() - t0), done_at_ms: round(performance.now() - campaignStart) });
  }
  return {
    agent,
    tasks,
    edit_ms: round(performance.now() - started),
    done_at_ms: round(performance.now() - campaignStart),
  };
}

async function runScenarioA({ rep, ctx, repId, workDir }) {
  const repoDir = path.join(workDir, 'repo');
  const log = new CommandLog();
  const sampler = new ProcessSampler({ dir: path.join(workDir, 'sampler'), psPath: ctx.ps.path, scriptPath: ctx.samplerScript, intervalMs: 250 });
  const tracker = new Tracker(sampler, ctx.sanitize);
  const notes = [];
  let setup = null;
  let samplerResult = null;
  let phaseStarted = false;

  try {
    sampler.start();
    await sleep(150);

    setup = await initRepo({ fixtureDir: ctx.fixtureDir, repoDir, log, tracker, sanitize: ctx.sanitize });
    const worktrees = {};
    for (const agent of AGENTS) {
      const dir = path.join(workDir, agent);
      await runOk('git', ['worktree', 'add', '-q', dir, '-b', agent], {
        cwd: repoDir,
        env: GIT_BASE_ENV,
        log,
        tracker,
        label: `git-worktree-add-${agent}`,
        sanitize: ctx.sanitize,
      });
      worktrees[agent] = dir;
    }
    const fixtureFiles = listFiles(ctx.fixtureDir).filter((rel) => !rel.startsWith('target/'));
    const baseContents = readContents(ctx.fixtureDir, fixtureFiles);
    const markersByTask = baseMarkers(baseContents);

    const hostBefore = lightHostSample();
    const nodeRssBefore = round(process.memoryUsage().rss / (1024 * 1024), 1);
    const campaignStart = performance.now();
    phaseStarted = true;

    const agentResults = await Promise.all(
      AGENTS.map((agent) =>
        agentEditA({ agent, worktreeDir: worktrees[agent], log, tracker, sanitize: ctx.sanitize, campaignStart }),
      ),
    );
    const editPhaseMs = round(performance.now() - campaignStart);

    // Intégration séquentielle des branches dans `integration` (merges réels).
    const merges = [];
    const conflicts = [];
    let conflictResolutionMs = 0;
    const mergeDoneAt = {};
    for (const agent of AGENTS) {
      const mergeStart = performance.now();
      const res = await run('git', ['merge', '--no-ff', '-m', `merge ${agent}`, agent], {
        cwd: repoDir,
        env: GIT_BASE_ENV,
        log,
        tracker,
        label: `git-merge-${agent}`,
        sanitize: ctx.sanitize,
      });
      const record = {
        agent,
        command_ms: round(performance.now() - mergeStart),
        exit_code: res.code,
        stdout_tail: tail(res.stdout, 2000),
        stderr_tail: tail(res.stderr, 2000),
      };
      if (res.code !== 0) {
        const unmerged = await run('git', ['diff', '--name-only', '--diff-filter=U'], {
          cwd: repoDir,
          env: GIT_BASE_ENV,
          log,
          tracker,
          label: 'git-unmerged',
          sanitize: ctx.sanitize,
        });
        const files = unmerged.stdout
          .split(/\r?\n/)
          .map((line) => line.trim())
          .filter(Boolean);
        record.unmerged_files = files;
        const tResolve = performance.now();
        for (const file of files) {
          // Politique pré-enregistrée : le côté déjà intégré gagne (même
          // résultat que le coordinateur, qui refuse le second).
          await runOk('git', ['checkout', '--ours', '--', file], {
            cwd: repoDir,
            env: GIT_BASE_ENV,
            log,
            tracker,
            label: 'git-resolve-ours',
            sanitize: ctx.sanitize,
          });
          await runOk('git', ['add', '--', file], { cwd: repoDir, env: GIT_BASE_ENV, log, tracker, label: 'git-add-resolved', sanitize: ctx.sanitize });
        }
        await runOk('git', ['commit', '-q', '--no-edit'], {
          cwd: repoDir,
          env: GIT_BASE_ENV,
          log,
          tracker,
          label: 'git-commit-merge',
          sanitize: ctx.sanitize,
        });
        record.resolution_ms = round(performance.now() - tResolve);
        conflictResolutionMs += record.resolution_ms;
        record.resolution = 'keep-integrated-side (checkout --ours puis commit de merge)';
        conflicts.push(record);
      }
      merges.push(record);
      mergeDoneAt[agent] = round(performance.now() - campaignStart);
    }
    const integrationPhaseMs = round(performance.now() - campaignStart - editPhaseMs);

    const statusCleanBeforeTests = await statusPorcelain(repoDir, log, tracker);
    const testResult = await runCargoTest({ repoDir, log, tracker, sanitize: ctx.sanitize });
    const tTotal = round(performance.now() - campaignStart);

    const expected = referenceContents(baseContents, expectedTaskOrder('A'));
    const content = await verifyContents({ repoDir, expected, fixtureFiles });
    const markerStates = markerCheck({ repoDir, markersByTask });
    const accepted = acceptedTaskIds('A');
    const independent = independentTaskIds();
    const lostAccepted = accepted.filter((id) => !markerStates[id].present);
    const lostIndependent = independent.filter((id) => !markerStates[id].present);
    const winnerMarkers = markerStates[WINNER.A];
    const loserMarkers = markerStates.t6;

    const gitInfo = await gitCapture(repoDir, log, tracker);
    const counts = await run('git', ['rev-list', '--count', `${setup.base_commit}..HEAD`], {
      cwd: repoDir,
      env: GIT_BASE_ENV,
      log,
      tracker,
      label: 'git-rev-list',
      sanitize: ctx.sanitize,
    });
    const mergeCommits = await run('git', ['rev-list', '--count', '--merges', `${setup.base_commit}..HEAD`], {
      cwd: repoDir,
      env: GIT_BASE_ENV,
      log,
      tracker,
      label: 'git-rev-list-merges',
      sanitize: ctx.sanitize,
    });

    const hostAfter = lightHostSample();
    const nodeRssAfter = round(process.memoryUsage().rss / (1024 * 1024), 1);
    const sizes = measureSizes(repoDir);
    samplerResult = await sampler.stop();

    const agentWaits = {};
    for (const agentResult of agentResults) {
      const wait = round(Math.max(0, mergeDoneAt[agentResult.agent] - agentResult.done_at_ms));
      agentWaits[agentResult.agent] = wait;
    }

    const result = {
      scenario: 'A',
      rep,
      rep_id: repId,
      ok: true,
      rebuilds: 1,
      timings: {
        setup_ms: setup.init_ms,
        edit_phase_ms: editPhaseMs,
        integration_phase_ms: integrationPhaseMs,
        test_ms: testResult.ms,
        t_total_ms: tTotal,
      },
      agents: agentResults,
      agent_waits_ms: agentWaits,
      agent_wait_total_ms: round(Object.values(agentWaits).reduce((a, b) => a + b, 0)),
      conflicts: {
        count: conflicts.length,
        resolution_ms: conflictResolutionMs,
        detection: 'git merge (conflit textuel sur src/delta.rs)',
        details: conflicts,
      },
      writes: {
        kind: 'worktree-per-agent',
        worktrees: AGENTS.length,
        agent_commits: agentResults.reduce((acc, a) => acc + a.tasks.length, 0),
        merge_commits: mergeCommits.code === 0 ? Number(mergeCommits.stdout.trim()) : null,
        integration_commits: counts.code === 0 ? Number(counts.stdout.trim()) : null,
      },
      tests: testResult,
      lost: {
        independent_missing: lostIndependent,
        accepted_missing: lostAccepted,
        silent_overwrites: 0,
        conflict_winner_present: winnerMarkers.present,
        rejected_edit_absent: !loserMarkers.present,
      },
      verification: {
        content_ok: content.ok,
        mismatches: content.mismatches,
        markers: markerStates,
        status_clean_before_tests: statusCleanBeforeTests === '',
        status_clean_after_tests: (await statusPorcelain(repoDir, log, tracker)) === '',
        git: {
          ...gitInfo,
          worktree_list: ctx.sanitize(gitInfo.worktree_list),
          agent_worktree_paths: Object.fromEntries(Object.entries(worktrees).map(([k, v]) => [k, ctx.sanitize(v)])),
          integration_commits: counts.code === 0 ? Number(counts.stdout.trim()) : null,
          merge_commits: mergeCommits.code === 0 ? Number(mergeCommits.stdout.trim()) : null,
        },
      },
      resources: {
        sampler: samplerResult,
        host_before: hostBefore,
        host_after: hostAfter,
        cpu_busy_pct_over_rep: cpuBusyPct(hostBefore, hostAfter),
        node_rss_mb_before: nodeRssBefore,
        node_rss_mb_after: nodeRssAfter,
      },
      sizes,
      notes,
      commands: log.entries,
      commands_dropped: log.dropped,
    };
    result.functional_success =
      result.tests.green &&
      content.ok &&
      lostIndependent.length === 0 &&
      lostAccepted.length === 0 &&
      result.verification.status_clean_after_tests &&
      winnerMarkers.present &&
      !loserMarkers.present;
    return result;
  } finally {
    tracker.remove(process.pid);
    if (phaseStarted && sampler && !samplerResult) {
      try {
        await sampler.stop({ timeoutMs: 4000 });
      } catch {
        /* ignore */
      }
    }
  }
}

// --- scénario B ------------------------------------------------------------

async function agentProposeB({ agent, baseContents, baseSnapshot, proposalsDir, baseHashes, log, tracker, sanitize, campaignStart, proposalIndex }) {
  const started = performance.now();
  const tasks = [];
  for (const task of tasksOf(agent)) {
    const t0 = performance.now();
    const { patch, targetPaths, contents } = buildUnifiedPatch(baseContents, task.ops);
    const expectedHashes = {};
    for (const rel of targetPaths) {
      const hash = baseHashes[rel];
      if (!hash) throw new Error(`hash de base absent pour ${rel}`);
      expectedHashes[rel] = hash;
    }
    const proposal = {
      schema_version: 1,
      agent_id: agent,
      task_id: task.id,
      base_snapshot_id: baseSnapshot.snapshot_id,
      target_paths: targetPaths,
      target_symbols: task.symbols,
      expected_hashes: expectedHashes,
      declared_dependencies: [],
      impacted_contracts: [],
      unified_patch: patch,
      note: `S8 deterministic scripted editor; task ${task.id}`,
    };
    const fileName = `${String(proposalIndex()).padStart(2, '0')}-${agent}-${task.id}.json`;
    const filePath = path.join(proposalsDir, fileName);
    writeJsonAtomic(filePath, proposal);
    tasks.push({
      id: task.id,
      ms: round(performance.now() - t0),
      proposal_file: fileName,
      proposal_sha256: sha256File(filePath),
      target_paths: targetPaths,
      patch_bytes: patch.length,
      proposed_contents_sha256: Object.fromEntries(targetPaths.map((rel) => [rel, sha256OfText(contents[rel])])),
      done_at_ms: round(performance.now() - campaignStart),
    });
  }
  return { agent, tasks, edit_ms: round(performance.now() - started), done_at_ms: round(performance.now() - campaignStart) };
}

function sha256OfText(text) {
  return sha256Hex(Buffer.from(text, 'utf8'));
}

function parseCoordinatorJson(stdout) {
  const index = stdout.indexOf('{');
  if (index < 0) throw new Error(`sortie coordinateur sans JSON:\n${stdout.slice(0, 2000)}`);
  const payload = JSON.parse(stdout.slice(index));
  const committedLine = stdout
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.startsWith('committed '))
    .pop();
  return { payload, committedLine };
}

function readJsonl(file) {
  if (!fs.existsSync(file)) return [];
  return fs
    .readFileSync(file, 'utf8')
    .split(/\r?\n/)
    .filter((line) => line.trim().length > 0)
    .map((line) => {
      try {
        return JSON.parse(line);
      } catch {
        return { parse_error: true, raw: line.slice(0, 300) };
      }
    });
}

async function runScenarioB({ rep, ctx, repId, workDir }) {
  const repoDir = path.join(workDir, 'repo');
  const proposalsDir = path.join(workDir, 'proposals');
  const log = new CommandLog();
  const sampler = new ProcessSampler({ dir: path.join(workDir, 'sampler'), psPath: ctx.ps.path, scriptPath: ctx.samplerScript, intervalMs: 250 });
  const tracker = new Tracker(sampler, ctx.sanitize);
  let setup = null;
  let samplerResult = null;
  let phaseStarted = false;
  let baseSnapshot = null;
  let baseHashes = null;

  try {
    sampler.start();
    await sleep(150);
    setup = await initRepo({ fixtureDir: ctx.fixtureDir, repoDir, log, tracker, sanitize: ctx.sanitize });
    fs.mkdirSync(proposalsDir, { recursive: true });

    const baseRes = await run(ctx.coordinatorExe, ['base', '--workspace', repoDir], {
      log,
      tracker,
      label: 'coordinator-base',
      sanitize: ctx.sanitize,
      timeoutMs: 120000,
    });
    if (baseRes.code !== 0) throw new Error(`ravum-collab base a échoué: ${baseRes.stderr}`);
    baseSnapshot = JSON.parse(baseRes.stdout.trim());
    baseHashes = baseSnapshot.files;
    const fixtureFiles = listFiles(ctx.fixtureDir).filter((rel) => !rel.startsWith('target/'));
    const baseContents = readContents(repoDir, fixtureFiles);
    const markersByTask = baseMarkers(baseContents);

    const hostBefore = lightHostSample();
    const nodeRssBefore = round(process.memoryUsage().rss / (1024 * 1024), 1);
    const campaignStart = performance.now();
    phaseStarted = true;

    let proposalCounter = 1;
    const proposalIndex = () => proposalCounter++;
    const agentResults = await Promise.all(
      AGENTS.map((agent) =>
        agentProposeB({
          agent,
          baseContents,
          baseSnapshot,
          proposalsDir,
          baseHashes,
          log,
          tracker,
          sanitize: ctx.sanitize,
          campaignStart,
          proposalIndex,
        }),
      ),
    );
    const editPhaseMs = round(performance.now() - campaignStart);
    const workspaceUntouchedBeforePublish = await statusPorcelain(repoDir, log, tracker);

    // Intégration : décisions + publication transactionnelle par le seul GitWriter.
    const coordinatorStart = performance.now();
    const journalPath = path.join(workDir, 'decisions.jsonl');
    const pubJournalPath = path.join(workDir, 'publications.jsonl');
    const coordRes = await run(
      ctx.coordinatorExe,
      [
        'integrate',
        '--workspace',
        repoDir,
        '--proposals',
        proposalsDir,
        '--journal',
        journalPath,
        '--publication-journal',
        pubJournalPath,
        '--commit',
        '--json',
        '--author',
        'ravum-coordinator',
        '--email',
        'coordinator@ravum.invalid',
      ],
      { log, tracker, label: 'coordinator-integrate', sanitize: ctx.sanitize, timeoutMs: 600000, maxOut: 1024 * 1024 },
    );
    const coordinatorMs = round(performance.now() - coordinatorStart);
    const integrationPhaseMs = coordinatorMs;
    const coordinatorTimestamp = round(performance.now() - campaignStart);
    if (coordRes.code !== 0) {
      throw new Error(`ravum-collab integrate a échoué (code ${coordRes.code}):\nstderr:\n${coordRes.stderr}\nstdout:\n${tail(coordRes.stdout, 6000)}`);
    }
    const { payload, committedLine } = parseCoordinatorJson(coordRes.stdout);
    const decisions = payload.decisions || [];
    const decisionByTask = {};
    for (const decision of decisions) {
      const taskId = String(decision.task_id || decision.proposal_id || '').split('/').pop();
      decisionByTask[taskId] = decision;
    }
    const summary = payload.summary || {};
    const statusCleanAfterPublish = await statusPorcelain(repoDir, log, tracker);

    const gitAfter = await gitCapture(repoDir, log, tracker);
    const head = gitAfter.head;
    const commitCount = await run('git', ['rev-list', '--count', `${setup.base_commit}..HEAD`], {
      cwd: repoDir,
      env: GIT_BASE_ENV,
      log,
      tracker,
      label: 'git-rev-list',
      sanitize: ctx.sanitize,
    });
    const commitPaths = await run('git', ['show', '--name-only', '--format=', head], {
      cwd: repoDir,
      env: GIT_BASE_ENV,
      log,
      tracker,
      label: 'git-show-paths',
      sanitize: ctx.sanitize,
    });
    const commitMessage = await run('git', ['log', '-1', '--format=%H%n%P%n%an%n%ae%n%s%n%B'], {
      cwd: repoDir,
      env: GIT_BASE_ENV,
      log,
      tracker,
      label: 'git-log-commit',
      sanitize: ctx.sanitize,
    });
    const publicationJournal = readJsonl(pubJournalPath);
    const decisionJournal = readJsonl(journalPath);
    const publicationIds = [...new Set(publicationJournal.map((line) => line.publication_id).filter(Boolean))];
    const publishedRecords = publicationJournal.filter((line) => String(line.status || '').toUpperCase() === 'PUBLISHED');
    const trailersPresent = ['Ravum-Proposal', 'Ravum-Agent'].filter((trailer) => commitMessage.stdout.includes(`${trailer}:`));

    const testResult = await runCargoTest({ repoDir, log, tracker, sanitize: ctx.sanitize });
    const tTotal = round(performance.now() - campaignStart);

    const expected = referenceContents(baseContents, expectedTaskOrder('B'));
    const content = await verifyContents({ repoDir, expected, fixtureFiles });
    const markerStates = markerCheck({ repoDir, markersByTask });
    const accepted = acceptedTaskIds('B');
    const independent = independentTaskIds();
    const lostAccepted = accepted.filter((id) => !markerStates[id].present);
    const lostIndependent = independent.filter((id) => !markerStates[id].present);

    const hostAfter = lightHostSample();
    const nodeRssAfter = round(process.memoryUsage().rss / (1024 * 1024), 1);
    const sizes = measureSizes(repoDir);
    samplerResult = await sampler.stop();

    const agentWaits = {};
    for (const agentResult of agentResults) {
      agentWaits[agentResult.agent] = round(Math.max(0, coordinatorTimestamp - agentResult.done_at_ms));
    }

    const result = {
      scenario: 'B',
      rep,
      rep_id: repId,
      ok: true,
      rebuilds: 1,
      timings: {
        setup_ms: setup.init_ms,
        edit_phase_ms: editPhaseMs,
        integration_phase_ms: integrationPhaseMs,
        test_ms: testResult.ms,
        t_total_ms: tTotal,
      },
      agents: agentResults,
      agent_waits_ms: agentWaits,
      agent_wait_total_ms: round(Object.values(agentWaits).reduce((a, b) => a + b, 0)),
      coordinator: {
        base_snapshot_id: baseSnapshot.snapshot_id,
        base_file_count: Object.keys(baseHashes).length,
        decisions_summary: summary,
        decisions,
        decision_by_task: decisionByTask,
        stdout_tail: tail(coordRes.stdout, 6000),
        stderr_tail: tail(coordRes.stderr, 2000),
        committed_line: committedLine ?? null,
        journal_entries: decisionJournal.length,
        publication_journal_records: publicationJournal.length,
        publication_ids: publicationIds,
        published_records: publishedRecords.map((record) => ({
          publication_id: record.publication_id,
          status: record.status,
          commit: record.commit ?? record.new_commit ?? null,
          reference: record.reference ?? null,
        })),
      },
      publication: {
        head,
        parent: (commitMessage.stdout.split(/\r?\n/)[1] || '').trim(),
        author_line: (commitMessage.stdout.split(/\r?\n/)[2] || '').trim(),
        email_line: (commitMessage.stdout.split(/\r?\n/)[3] || '').trim(),
        subject: (commitMessage.stdout.split(/\r?\n/)[4] || '').trim(),
        body: tail(commitMessage.stdout.split(/\r?\n/).slice(5).join('\n'), 2500),
        commit_count_since_base: commitCount.code === 0 ? Number(commitCount.stdout.trim()) : null,
        paths: commitPaths.stdout
          .split(/\r?\n/)
          .map((line) => line.trim())
          .filter(Boolean),
        trailers_present: trailersPresent,
        workspace_untouched_before_publish: workspaceUntouchedBeforePublish === '',
        status_clean_after_publish: statusCleanAfterPublish === '',
      },
      conflicts: {
        count: Number(summary.conflict ?? 0),
        resolution_ms: 0,
        detection: 'coordinateur Ravum (CONFLICT, refus sans écriture)',
        details: decisions.filter((decision) => decision.decision === 'CONFLICT'),
      },
      writes: {
        kind: 'shared-workspace-coordinator',
        publication_commits: commitCount.code === 0 ? Number(commitCount.stdout.trim()) : null,
        paths_published: commitPaths.stdout.split(/\r?\n/).filter((line) => line.trim()).length,
        coordinator_decisions: decisions.length,
      },
      tests: testResult,
      lost: {
        independent_missing: lostIndependent,
        accepted_missing: lostAccepted,
        silent_overwrites: 0,
        conflict_winner_present: markerStates[WINNER.B].present,
        rejected_edit_absent: !markerStates.t6.present,
      },
      verification: {
        content_ok: content.ok,
        mismatches: content.mismatches,
        markers: markerStates,
        status_clean_before_tests: statusCleanAfterPublish === '',
        status_clean_after_tests: (await statusPorcelain(repoDir, log, tracker)) === '',
        git: {
          ...gitAfter,
          worktree_list: ctx.sanitize(gitAfter.worktree_list),
          integration_commits: commitCount.code === 0 ? Number(commitCount.stdout.trim()) : null,
          commit_paths: commitPaths.stdout.split(/\r?\n/).map((line) => line.trim()).filter(Boolean),
        },
      },
      resources: {
        sampler: samplerResult,
        host_before: hostBefore,
        host_after: hostAfter,
        cpu_busy_pct_over_rep: cpuBusyPct(hostBefore, hostAfter),
        node_rss_mb_before: nodeRssBefore,
        node_rss_mb_after: nodeRssAfter,
      },
      sizes,
      notes: [
        'Les agents B ne touchent jamais le workspace : ils produisent des ChangeProposal JSON hors dépôt.',
        'La publication est effectuée par le GitWriter transactionnel du coordinateur, en une seule publication.',
        'Le conflit t4/t6 est un refus CONFLICT attendu : c’est un résultat mesuré, pas un échec du harnais.',
      ],
      commands: log.entries,
      commands_dropped: log.dropped,
    };
    result.functional_success =
      result.tests.green &&
      content.ok &&
      lostIndependent.length === 0 &&
      lostAccepted.length === 0 &&
      result.verification.status_clean_after_tests &&
      trailersPresent.length === 2 &&
      result.publication.workspace_untouched_before_publish &&
      result.publication.status_clean_after_publish &&
      markerStates[WINNER.B].present &&
      !markerStates.t6.present;
    return result;
  } finally {
    tracker.remove(process.pid);
    if (phaseStarted && sampler && !samplerResult) {
      try {
        await sampler.stop({ timeoutMs: 4000 });
      } catch {
        /* ignore */
      }
    }
  }
}

// --- scénario C ------------------------------------------------------------

async function runScenarioC({ rep, ctx, repId, workDir }) {
  const repoDir = path.join(workDir, 'repo');
  const log = new CommandLog();
  const sampler = new ProcessSampler({ dir: path.join(workDir, 'sampler'), psPath: ctx.ps.path, scriptPath: ctx.samplerScript, intervalMs: 250 });
  const tracker = new Tracker(sampler, ctx.sanitize);
  let setup = null;
  let samplerResult = null;
  let phaseStarted = false;

  try {
    sampler.start();
    await sleep(150);
    setup = await initRepo({ fixtureDir: ctx.fixtureDir, repoDir, log, tracker, sanitize: ctx.sanitize });
    const fixtureFiles = listFiles(ctx.fixtureDir).filter((rel) => !rel.startsWith('target/'));
    const baseContents = readContents(repoDir, fixtureFiles);
    const markersByTask = baseMarkers(baseContents);

    const hostBefore = lightHostSample();
    const nodeRssBefore = round(process.memoryUsage().rss / (1024 * 1024), 1);
    const campaignStart = performance.now();
    phaseStarted = true;

    const taskRecords = [];
    const agentWaits = Object.fromEntries(AGENTS.map((agent) => [agent, 0]));
    let previousEnd = 0;
    for (const task of TASKS) {
      const taskStart = round(performance.now() - campaignStart);
      const wait = round(Math.max(0, taskStart - previousEnd));
      agentWaits[task.agent] = round(agentWaits[task.agent] + wait);
      const t0 = performance.now();
      applyOpsToDir(repoDir, task.ops);
      await runOk('git', ['add', '-A'], { cwd: repoDir, env: GIT_BASE_ENV, log, tracker, label: 'git-add', sanitize: ctx.sanitize });
      await runOk('git', ['commit', '-q', '-m', `${task.id}: ${task.title}`], {
        cwd: repoDir,
        env: GIT_BASE_ENV,
        log,
        tracker,
        label: 'git-commit',
        sanitize: ctx.sanitize,
      });
      const taskMs = round(performance.now() - t0);
      const taskEnd = round(performance.now() - campaignStart);
      taskRecords.push({ id: task.id, agent: task.agent, start_at_ms: taskStart, end_at_ms: taskEnd, wait_ms: wait, ms: taskMs });
      previousEnd = taskEnd;
    }
    const editPhaseMs = round(performance.now() - campaignStart);
    const statusCleanBeforeTests = await statusPorcelain(repoDir, log, tracker);
    const testResult = await runCargoTest({ repoDir, log, tracker, sanitize: ctx.sanitize });
    const tTotal = round(performance.now() - campaignStart);

    const expected = referenceContents(baseContents, expectedTaskOrder('C'));
    const content = await verifyContents({ repoDir, expected, fixtureFiles });
    const markerStates = markerCheck({ repoDir, markersByTask });
    const accepted = acceptedTaskIds('C');
    const independent = independentTaskIds();
    const lostAccepted = accepted.filter((id) => !markerStates[id].present);
    // t4 est ici le perdant PRÉ-VU du conflit contrôlé : sa disparition est
    // mesurée comme « changement accepté perdu / écrasement silencieux », pas
    // comme une édition indépendante perdue.
    const lostIndependent = independentTaskIds()
      .filter((id) => id !== 't4')
      .filter((id) => !markerStates[id].present);
    const silentOverwrites = lostAccepted.filter((id) => id === 't4');

    const gitInfo = await gitCapture(repoDir, log, tracker);
    const counts = await run('git', ['rev-list', '--count', `${setup.base_commit}..HEAD`], {
      cwd: repoDir,
      env: GIT_BASE_ENV,
      log,
      tracker,
      label: 'git-rev-list',
      sanitize: ctx.sanitize,
    });
    const commitList = await run('git', ['log', '--format=%h %s', `${setup.base_commit}..HEAD`], {
      cwd: repoDir,
      env: GIT_BASE_ENV,
      log,
      tracker,
      label: 'git-log-list',
      sanitize: ctx.sanitize,
    });

    const hostAfter = lightHostSample();
    const nodeRssAfter = round(process.memoryUsage().rss / (1024 * 1024), 1);
    const sizes = measureSizes(repoDir);
    samplerResult = await sampler.stop();

    const result = {
      scenario: 'C',
      rep,
      rep_id: repId,
      ok: true,
      rebuilds: 1,
      timings: {
        setup_ms: setup.init_ms,
        edit_phase_ms: editPhaseMs,
        integration_phase_ms: 0,
        test_ms: testResult.ms,
        t_total_ms: tTotal,
      },
      agents: AGENTS.map((agent) => ({
        agent,
        tasks: taskRecords.filter((record) => record.agent === agent),
        edit_ms: round(
          taskRecords
            .filter((record) => record.agent === agent)
            .reduce((acc, record) => acc + record.ms, 0),
        ),
      })),
      agent_waits_ms: agentWaits,
      agent_wait_total_ms: round(Object.values(agentWaits).reduce((a, b) => a + b, 0)),
      conflicts: {
        count: 0,
        resolution_ms: 0,
        detection: 'aucune (agents séquentiels, aucune détection de conflit)',
        details: [],
      },
      writes: {
        kind: 'sequential-single-branch',
        commits: counts.code === 0 ? Number(counts.stdout.trim()) : null,
        commit_list: tail(commitList.stdout, 2000),
      },
      tests: testResult,
      lost: {
        independent_missing: lostIndependent,
        accepted_missing: lostAccepted,
        silent_overwrites: silentOverwrites.length,
        silent_overwrite_tasks: silentOverwrites,
        conflict_winner_present: markerStates[WINNER.C].present,
        rejected_edit_absent: !markerStates.t4.present,
      },
      verification: {
        content_ok: content.ok,
        mismatches: content.mismatches,
        markers: markerStates,
        status_clean_before_tests: statusCleanBeforeTests === '',
        status_clean_after_tests: (await statusPorcelain(repoDir, log, tracker)) === '',
        git: {
          ...gitInfo,
          worktree_list: ctx.sanitize(gitInfo.worktree_list),
          integration_commits: counts.code === 0 ? Number(counts.stdout.trim()) : null,
          commit_list: tail(commitList.stdout, 2000),
        },
      },
      resources: {
        sampler: samplerResult,
        host_before: hostBefore,
        host_after: hostAfter,
        cpu_busy_pct_over_rep: cpuBusyPct(hostBefore, hostAfter),
        node_rss_mb_before: nodeRssBefore,
        node_rss_mb_after: nodeRssAfter,
      },
      sizes,
      notes: [
        'Contrôle : les 6 éditions sont appliquées l’une après l’autre sur la branche unique, sans aucun contrôle de concurrence.',
        't6 écrase silencieusement t4 : la perte est mesurée, pas niée.',
      ],
      commands: log.entries,
      commands_dropped: log.dropped,
    };
    result.functional_success =
      result.tests.green &&
      content.ok &&
      lostIndependent.length === 0 &&
      lostAccepted.length === 0 &&
      result.verification.status_clean_after_tests &&
      markerStates[WINNER.C].present &&
      !markerStates.t4.present;
    return result;
  } finally {
    tracker.remove(process.pid);
    if (phaseStarted && sampler && !samplerResult) {
      try {
        await sampler.stop({ timeoutMs: 4000 });
      } catch {
        /* ignore */
      }
    }
  }
}

// --- point d'entrée --------------------------------------------------------

export async function runRep({ scenario, rep, scheduleIndex, ctx }) {
  const repId = `${scenario}${rep}`;
  const workDir = path.join(ctx.replicaRoot, repId);
  fs.mkdirSync(workDir, { recursive: true });
  const startedAt = Date.now();
  let result;
  try {
    if (scenario === 'A') result = await runScenarioA({ rep, ctx, repId, workDir });
    else if (scenario === 'B') result = await runScenarioB({ rep, ctx, repId, workDir });
    else if (scenario === 'C') result = await runScenarioC({ rep, ctx, repId, workDir });
    else throw new Error(`scénario inconnu: ${scenario}`);
  } finally {
    if (!ctx.keep) {
      try {
        fs.rmSync(workDir, { recursive: true, force: true, maxRetries: 15, retryDelay: 200 });
      } catch (error) {
        // Nettoyage best-effort : un fichier verrouillé ne doit pas masquer un résultat.
        if (result) result.cleanup_error = String(error);
      }
    } else if (result) {
      result.kept_work_dir = workDir;
    }
  }
  result.schedule_index = scheduleIndex;
  result.started_at_epoch_ms = startedAt;
  result.finished_at_epoch_ms = Date.now();
  result.work_dir = ctx.sanitize(workDir);
  return result;
}
